//! 输入法进程自己的小对话框：**非模态**窗口，结果走回调。
//!
//! 这里不能用 `NSAlert.runModal` / `NSOpenPanel.runModal`：模态框把主线程关进模态 run loop，
//! IMK 的按键派发跟着停摆——框里一个字符也收不到，而且**别的应用也一起打不了字**
//! （按键都路由给青简，青简的主线程却卡在模态循环里；切到别的输入法又能打，因为不再路由给它）。
//! 非模态窗口只是 show 一下，主 run loop 照常跑，青简照常给框里的输入框组句，候选窗也照常出。
//!
//! 一次只开一个框：再开一个时，旧的那个按「取消」结算。

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSBorderType, NSButton,
    NSEventModifierFlags, NSFont, NSMenu, NSMenuItem, NSScrollView, NSTextField, NSTextView,
    NSView, NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{
    NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString,
};

/// 内边距与控件尺寸。窗口不大，手动摆 frame 就够，不引 AutoLayout。
const MARGIN: f64 = 20.0;
const WIDTH: f64 = 420.0;
const INNER: f64 = WIDTH - 2.0 * MARGIN;
const FIELD_HEIGHT: f64 = 24.0;
const BUTTON_HEIGHT: f64 = 28.0;
/// 多行输入区的高度；窗口跟着变高，说明文字挂在它上面。
const AREA_HEIGHT: f64 = 120.0;
/// 说明文字占的高度：多行框的说明有两行，得留够。
const MESSAGE_HEIGHT: f64 = 48.0;
/// 多行框的宽度：给滚动条让出一点地方，否则文字贴着边框。
const AREA_WIDTH: f64 = INNER - 16.0;

thread_local! {
    /// 当前开着的那个框；关窗（按钮、关闭键、⌘W）时取走结算。
    static CURRENT: RefCell<Option<Dialog>> = const { RefCell::new(None) };
}

/// 输入区的种类，`prefill` 是预填内容。
enum Kind<'a> {
    /// 只有说明和一个「好」的提示框
    Notice,
    /// 单行输入框
    Line(&'a str),
    /// 多行输入区：一行一条，回车换行
    Area(&'a str),
}

/// 建出来的输入控件，取值时区分类型。
enum Field {
    Line(Retained<NSTextField>),
    Area(Retained<NSTextView>),
}

impl Field {
    fn text(&self) -> String {
        match self {
            Self::Line(field) => field.stringValue().to_string(),
            Self::Area(area) => area.string().to_string(),
        }
    }

    fn view(&self) -> &NSView {
        match self {
            Self::Line(field) => field,
            Self::Area(area) => area,
        }
    }
}

struct Dialog {
    window: Retained<NSWindow>,
    /// 窗口的 delegate 是弱引用，得由这里握着。
    target: Retained<DialogTarget>,
    field: Option<Field>,
    done: Box<dyn FnOnce(Option<String>)>,
}

define_class!(
    // SAFETY: NSObject 没有子类化要求；没有实现 Drop。
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "QingjianDialogTarget"]
    #[ivars = ()]
    /// 按钮的 target 兼窗口 delegate。选择器起专门的名字，免得撞上响应链里的同名方法。
    pub struct DialogTarget;

    impl DialogTarget {
        #[unsafe(method(dialogAccept:))]
        fn accept(&self, _sender: Option<&AnyObject>) {
            settle(true);
        }

        #[unsafe(method(dialogDismiss:))]
        fn dismiss(&self, _sender: Option<&AnyObject>) {
            settle(false);
        }

        #[unsafe(method(windowWillClose:))]
        fn window_will_close(&self, _notification: &NSNotification) {
            // 点关闭按钮、⌘W：按取消算。按钮那条路已经把状态取走了，到这里是空转。
            settle(false);
        }
    }

    unsafe impl NSObjectProtocol for DialogTarget {}
    unsafe impl NSWindowDelegate for DialogTarget {}
);

/// 一个带单行输入框的小对话框：`title` 标题、`message` 说明、`prefill` 预填内容、`ok_title` 确定按钮。
/// 确定且内容非空给 `Some(文字)`，取消、关窗或内容为空给 `None`。
pub fn prompt(
    title: &str,
    message: &str,
    prefill: &str,
    ok_title: &str,
    done: impl FnOnce(Option<String>) + 'static,
) {
    show(title, message, Kind::Line(prefill), ok_title, done);
}

/// 多行输入框：一行一条，整段文字原样交回，按行切由调用方决定。
///
/// 框里回车是换行（多行框的本分），所以确定键挂的是 **⌘↩**；按钮照旧可以点。
pub fn prompt_area(
    title: &str,
    message: &str,
    prefill: &str,
    ok_title: &str,
    done: impl FnOnce(Option<String>) + 'static,
) {
    show(title, message, Kind::Area(prefill), ok_title, done);
}

/// 只有一句说明和一个「好」的提示框（造词失败的原因等）。
pub fn notice(title: &str, message: &str) {
    show(title, message, Kind::Notice, "好", |_| {});
}

fn show(
    title: &str,
    message: &str,
    kind: Kind<'_>,
    ok_title: &str,
    done: impl FnOnce(Option<String>) + 'static,
) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    // 已经开着一个：先按取消收掉，别叠两个框抢状态槽
    settle(false);

    let target = DialogTarget::new(mtm);
    // 输入区占的高度：单行一个输入框，多行一整块滚动区，提示框没有
    let slot = match kind {
        Kind::Notice => 0.0,
        Kind::Line(_) => FIELD_HEIGHT + 10.0,
        Kind::Area(_) => AREA_HEIGHT + 10.0,
    };
    let input_bottom = MARGIN + BUTTON_HEIGHT + 12.0;
    let content_height = match kind {
        Kind::Notice => 132.0,
        Kind::Line(_) => 170.0,
        Kind::Area(_) => input_bottom + slot + MESSAGE_HEIGHT + MARGIN,
    };
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            mtm.alloc(),
            NSRect::new(NSPoint::ZERO, NSSize::new(WIDTH, content_height)),
            NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    window.setTitle(&NSString::from_str(title));
    // 程序建的窗口默认关窗即释放，我们还握着 Retained，必须关掉
    unsafe { window.setReleasedWhenClosed(false) };
    window.setDelegate(Some(ProtocolObject::from_ref(&*target)));

    let content = NSView::initWithFrame(
        mtm.alloc(),
        NSRect::new(NSPoint::ZERO, NSSize::new(WIDTH, content_height)),
    );
    window.setContentView(Some(&content));

    // 按钮在最下面：确定靠右，取消在它左边
    let ok = unsafe {
        NSButton::buttonWithTitle_target_action(
            &NSString::from_str(ok_title),
            Some(&*target),
            Some(sel!(dialogAccept:)),
            mtm,
        )
    };
    ok.setFrame(NSRect::new(
        NSPoint::new(WIDTH - MARGIN - 96.0, MARGIN),
        NSSize::new(96.0, BUTTON_HEIGHT),
    ));
    ok.setKeyEquivalent(&NSString::from_str("\r"));
    if matches!(kind, Kind::Area(_)) {
        // 多行框里回车归换行，确定改成 ⌘↩，免得刚打一行就被存掉
        ok.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
    }
    content.addSubview(&ok);
    if !matches!(kind, Kind::Notice) {
        let cancel = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str("取消"),
                Some(&*target),
                Some(sel!(dialogDismiss:)),
                mtm,
            )
        };
        cancel.setFrame(NSRect::new(
            NSPoint::new(WIDTH - MARGIN - 96.0 - 8.0 - 76.0, MARGIN),
            NSSize::new(76.0, BUTTON_HEIGHT),
        ));
        cancel.setKeyEquivalent(&NSString::from_str("\u{1b}"));
        content.addSubview(&cancel);
    }

    // 输入框在按钮上面；没有输入框的提示框把说明文字拉满这一块
    let field = match kind {
        Kind::Notice => None,
        Kind::Line(value) => Some(Field::Line(line_field(mtm, value, input_bottom, &content))),
        Kind::Area(value) => Some(Field::Area(area_field(mtm, value, input_bottom, &content))),
    };
    let message_bottom = input_bottom + slot;
    let label = NSTextField::wrappingLabelWithString(&NSString::from_str(message), mtm);
    label.setFrame(NSRect::new(
        NSPoint::new(MARGIN, message_bottom),
        NSSize::new(INNER, content_height - message_bottom - MARGIN),
    ));
    content.addSubview(&label);

    // 落焦点在输入框上：窗口还没上屏也设得动，成为 key window 后就是它收键盘
    if let Some(field) = &field {
        let view = field.view();
        window.setInitialFirstResponder(Some(view));
        window.makeFirstResponder(Some(view));
    }

    CURRENT.with(|current| {
        *current.borrow_mut() = Some(Dialog {
            window: window.clone(),
            target,
            field,
            done: Box::new(done),
        });
    });

    // Accessory 激活策略 + 编辑主菜单（⌘C / ⌘V 靠它），再激活、置 key、落焦点
    install_edit_menu(mtm);
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
    window.center();
    window.makeKeyAndOrderFront(None);
}

/// 单行输入框。
fn line_field(
    mtm: MainThreadMarker,
    value: &str,
    bottom: f64,
    content: &NSView,
) -> Retained<NSTextField> {
    let field = NSTextField::initWithFrame(
        mtm.alloc(),
        NSRect::new(
            NSPoint::new(MARGIN, bottom),
            NSSize::new(INNER, FIELD_HEIGHT),
        ),
    );
    field.setStringValue(&NSString::from_str(value));
    field.setEditable(true);
    field.setSelectable(true);
    content.addSubview(&field);
    field
}

/// 多行输入区：`NSTextView` 放进滚动视图，行数多了可以滚。
fn area_field(
    mtm: MainThreadMarker,
    value: &str,
    bottom: f64,
    content: &NSView,
) -> Retained<NSTextView> {
    let scroll = NSScrollView::initWithFrame(
        mtm.alloc(),
        NSRect::new(
            NSPoint::new(MARGIN, bottom),
            NSSize::new(INNER, AREA_HEIGHT),
        ),
    );
    // 带边框：不然多行区和窗口底色分不开，用户看不出能往哪儿打
    scroll.setBorderType(NSBorderType::BezelBorder);
    scroll.setHasVerticalScroller(true);
    scroll.setAutohidesScrollers(true);

    let area = NSTextView::initWithFrame(
        mtm.alloc(),
        NSRect::new(NSPoint::ZERO, NSSize::new(AREA_WIDTH, AREA_HEIGHT)),
    );
    area.setString(&NSString::from_str(value));
    area.setEditable(true);
    area.setRichText(false);
    area.setFont(Some(&NSFont::systemFontOfSize(13.0)));
    // 高度随内容长、宽度钉死（按窗口宽折行），超出的交给滚动条
    area.setVerticallyResizable(true);
    area.setHorizontallyResizable(false);
    area.setMinSize(NSSize::new(0.0, AREA_HEIGHT));
    area.setMaxSize(NSSize::new(AREA_WIDTH, f64::MAX));
    area.setTextContainerInset(NSSize::new(4.0, 4.0));
    area.setAllowsUndo(true);
    // 预填的内容可能早就超过一屏：`setString` 不会触发自动长高，得让它自己量一次，
    // 否则文档视图还只有一屏高，后面的词滚不到
    area.sizeToFit();
    scroll.setDocumentView(Some(&area));

    content.addSubview(&scroll);
    area
}

/// 结算并关窗。`accept` 为真且内容非空时回调拿到文字，其余情况都是 `None`。
fn settle(accept: bool) {
    let Some(dialog) = CURRENT.with(|current| current.borrow_mut().take()) else {
        return;
    };
    let text = if accept {
        // 先结束编辑：组句还没上屏时让 IMK 把这段结掉，别读到一串没选的拼音
        dialog.window.makeFirstResponder(None);
        dialog.field.as_ref().map(Field::text).unwrap_or_default()
    } else {
        String::new()
    };
    let text = text.trim().to_owned();
    let answer = (accept && !text.is_empty()).then_some(text);
    let done = dialog.done;
    let _keep_target_alive = &dialog.target;
    dialog.window.close();
    done(answer);
}

impl DialogTarget {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

/// 给进程装一份只有「编辑」的主菜单。
///
/// 输入法是 `LSBackgroundOnly`，平时不显示菜单栏，也就没有主菜单；而 AppKit 文本框里的 ⌘X / ⌘C / ⌘V / ⌘A / ⌘Z
/// 是主菜单里对应条目的快捷键，菜单不存在这些键就没人响应——框里的密钥、词表粘不进去就是这个原因。
/// 菜单栏本身永远不会画出来（激活策略最多到 Accessory），只是让快捷键有地方落。重复调用只装一次。
fn install_edit_menu(mtm: MainThreadMarker) {
    let app = NSApplication::sharedApplication(mtm);
    if app.mainMenu().is_some() {
        return;
    }
    let edit = NSMenu::initWithTitle(mtm.alloc(), &NSString::from_str("编辑"));
    for (title, action, key) in [
        ("撤销", sel!(undo:), "z"),
        ("重做", sel!(redo:), "Z"),
        ("剪切", sel!(cut:), "x"),
        ("拷贝", sel!(copy:), "c"),
        ("粘贴", sel!(paste:), "v"),
        ("全选", sel!(selectAll:), "a"),
    ] {
        // SAFETY: 这些都是 NSResponder 的标准编辑选择器，target 为空时沿响应链送到当前编辑的文本框
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                mtm.alloc(),
                &NSString::from_str(title),
                Some(action),
                &NSString::from_str(key),
            )
        };
        edit.addItem(&item);
    }
    let main: Retained<NSMenu> = NSMenu::new(mtm);
    let holder = NSMenuItem::new(mtm);
    holder.setSubmenu(Some(&edit));
    main.addItem(&holder);
    app.setMainMenu(Some(&main));
}

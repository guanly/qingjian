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
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSButton, NSTextField,
    NSView, NSWindow, NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{
    NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString,
};

use crate::preferences::edit_menu;

/// 内边距与控件尺寸。窗口不大，手动摆 frame 就够，不引 AutoLayout。
const MARGIN: f64 = 20.0;
const WIDTH: f64 = 420.0;
const INNER: f64 = WIDTH - 2.0 * MARGIN;
const FIELD_HEIGHT: f64 = 24.0;
const BUTTON_HEIGHT: f64 = 28.0;

thread_local! {
    /// 当前开着的那个框；关窗（按钮、关闭键、⌘W）时取走结算。
    static CURRENT: RefCell<Option<Dialog>> = const { RefCell::new(None) };
}

struct Dialog {
    window: Retained<NSWindow>,
    /// 窗口的 delegate 是弱引用，得由这里握着。
    target: Retained<DialogTarget>,
    field: Option<Retained<NSTextField>>,
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

/// 一个带输入框的小对话框：`title` 标题、`message` 说明、`prefill` 预填内容、`ok_title` 确定按钮。
/// 确定且内容非空给 `Some(文字)`，取消、关窗或内容为空给 `None`。
pub fn prompt(
    title: &str,
    message: &str,
    prefill: &str,
    ok_title: &str,
    done: impl FnOnce(Option<String>) + 'static,
) {
    show(title, message, Some(prefill), ok_title, done);
}

/// 只有一句说明和一个「好」的提示框（造词失败的原因等）。
pub fn notice(title: &str, message: &str) {
    show(title, message, None, "好", |_| {});
}

fn show(
    title: &str,
    message: &str,
    prefill: Option<&str>,
    ok_title: &str,
    done: impl FnOnce(Option<String>) + 'static,
) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    // 已经开着一个：先按取消收掉，别叠两个框抢状态槽
    settle(false);

    let target = DialogTarget::new(mtm);
    let content_height = if prefill.is_some() { 170.0 } else { 132.0 };
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
    content.addSubview(&ok);
    if prefill.is_some() {
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
    let field = prefill.map(|value| {
        let field = NSTextField::initWithFrame(
            mtm.alloc(),
            NSRect::new(
                NSPoint::new(MARGIN, MARGIN + BUTTON_HEIGHT + 12.0),
                NSSize::new(INNER, FIELD_HEIGHT),
            ),
        );
        field.setStringValue(&NSString::from_str(value));
        field.setEditable(true);
        field.setSelectable(true);
        content.addSubview(&field);
        field
    });
    let message_bottom = field.as_ref().map_or(MARGIN + BUTTON_HEIGHT + 12.0, |_| {
        MARGIN + BUTTON_HEIGHT + 12.0 + FIELD_HEIGHT + 10.0
    });
    let label = NSTextField::wrappingLabelWithString(&NSString::from_str(message), mtm);
    label.setFrame(NSRect::new(
        NSPoint::new(MARGIN, message_bottom),
        NSSize::new(INNER, content_height - message_bottom - MARGIN),
    ));
    content.addSubview(&label);

    CURRENT.with(|current| {
        *current.borrow_mut() = Some(Dialog {
            window: window.clone(),
            target,
            field: field.clone(),
            done: Box::new(done),
        });
    });

    // 与偏好设置窗口同一套：Accessory + 编辑主菜单（⌘C / ⌘V 靠它），再激活、置 key、落焦点
    edit_menu::install(mtm);
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
    window.center();
    window.makeKeyAndOrderFront(None);
    if let Some(field) = &field {
        window.setInitialFirstResponder(Some(field));
        window.makeFirstResponder(Some(field));
    }
}

/// 结算并关窗。`accept` 为真且内容非空时回调拿到文字，其余情况都是 `None`。
fn settle(accept: bool) {
    let Some(dialog) = CURRENT.with(|current| current.borrow_mut().take()) else {
        return;
    };
    let text = if accept {
        // 先结束编辑：组句还没上屏时让 IMK 把这段结掉，别读到一串没选的拼音
        dialog.window.makeFirstResponder(None);
        dialog
            .field
            .as_ref()
            .map(|field| field.stringValue().to_string())
            .unwrap_or_default()
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

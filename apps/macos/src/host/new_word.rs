//! 造词框：输入一个中文词，读音与形码编码都由引擎算好。
//!
//! 用系统原生警告框收一个值，与候选译文修改框（`imk/controller/candidate_editor.rs`）同一套做法：
//! 输入法进程在 IMK 回调里跑在主线程，可以直接 `runModal`。
//!
//! 只收中文：拼音逐字从词库反查、五笔编码按当前形码方案的词码规则拼——用户不必再手打音节，
//! 也就不会出现「字数和音节数对不上」那类错误。

use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSApplication, NSApplicationActivationPolicy, NSTextField,
    NSView,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use qingjian_core::Engine;

/// 造词框的宽度与高度。
const WIDTH: f64 = 380.0;
const ROW: f64 = 24.0;

/// 弹造词框，返回要记成用户词的文字；取消给 `None`。
///
/// 预填「最近上屏的那一段」，用户直接回车就是造刚打的词，也可以改成别的。
pub fn ask(engine: &Engine) -> Option<String> {
    let mtm = MainThreadMarker::new()?;
    let prefill = engine
        .recent_commits(1)
        .first()
        .map(|(text, _)| text.clone())
        .unwrap_or_default();

    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str("造词"));
    alert.setInformativeText(&NSString::from_str(
        "只填中文词，拼音会自动算好；选了形码方案时连编码一起算，两种打法都能出来。",
    ));
    alert.addButtonWithTitle(&NSString::from_str("存为用户词"));
    alert.addButtonWithTitle(&NSString::from_str("取消"));

    let view = NSView::initWithFrame(
        mtm.alloc(),
        NSRect::new(NSPoint::ZERO, NSSize::new(WIDTH, ROW)),
    );
    let field = field(mtm, &prefill);
    view.addSubview(&field);
    alert.setAccessoryView(Some(&view));

    let window = alert.window();
    window.setInitialFirstResponder(Some(&field));
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
    window.makeKeyAndOrderFront(None);
    // 输入法进程平时不当前台应用，窗口成了 key window 之后焦点不一定落在输入框上
    window.makeFirstResponder(Some(&field));

    if alert.runModal() != NSAlertFirstButtonReturn {
        return None;
    }
    let text = field.stringValue().to_string();
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// 造词失败的原因再用一个框说清楚：notice 行只在候选窗可见，菜单操作时窗口可能根本没开。
pub fn report(reason: &str) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str("没有存上"));
    alert.setInformativeText(&NSString::from_str(reason));
    alert.addButtonWithTitle(&NSString::from_str("好"));
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
    alert.runModal();
}

fn field(mtm: MainThreadMarker, value: &str) -> objc2::rc::Retained<NSTextField> {
    let field = NSTextField::initWithFrame(
        mtm.alloc(),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(WIDTH, ROW)),
    );
    field.setStringValue(&NSString::from_str(value));
    field.setEditable(true);
    field.setSelectable(true);
    field
}

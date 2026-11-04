use objc2::MainThreadMarker;
use objc2_app_kit::{NSModalResponse, NSModalResponseOK, NSOpenPanel};
use objc2_foundation::NSString;
use std::cell::RefCell;
use std::path::PathBuf;

/// 弹系统的打开文件对话框选一个词库文件（TSV / Rime yaml / .qj），选完走回调，取消给 `None`。
///
/// 只在偏好设置窗口打开时调用（那时进程已是 Accessory，能出对话框）。
/// 用异步的 `beginWithCompletionHandler:` 而不是 `runModal`：模态 run loop 会卡住 IMK 的按键派发，
/// 面板开着的时候整个系统的输入都断（见 `crate::dialog` 的说明）。
pub fn choose_dictionary_file(done: impl FnOnce(Option<PathBuf>) + 'static) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setCanChooseFiles(true);
    panel.setCanChooseDirectories(false);
    panel.setAllowsMultipleSelection(false);
    panel.setMessage(Some(&NSString::from_str(
        "选择要导入的词库：青简 TSV、Rime .dict.yaml 或 .qj",
    )));
    panel.setPrompt(Some(&NSString::from_str("导入")));
    let done = RefCell::new(Some(done));
    let block: block2::RcBlock<dyn Fn(NSModalResponse)> = block2::RcBlock::new({
        // 面板由 block 握着，活到手回调执行完
        let panel = panel.clone();
        move |result| {
            let path = if result == NSModalResponseOK {
                panel
                    .URLs()
                    .firstObject()
                    .and_then(|url| url.path())
                    .map(|path| PathBuf::from(path.to_string()))
            } else {
                None
            };
            if let Some(done) = done.borrow_mut().take() {
                done(path);
            }
        }
    });
    panel.beginWithCompletionHandler(&block);
}

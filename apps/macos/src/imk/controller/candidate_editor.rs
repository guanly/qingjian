//! 候选修正输入框：收一段正确文本。
//!
//! 与造词框共用 [`crate::dialog`] 的非模态窗口（模态 `runModal` 会卡住 IMK 的按键派发，框里打不了字），
//! 结果走回调。

pub(super) fn edit_translation(
    current: &str,
    language: &str,
    done: impl FnOnce(Option<String>) + 'static,
) {
    tracing::info!(%language, "打开候选译文输入框");
    let message = format!("输入正确的{language}译文；保存后会优先于内置释义显示。");
    crate::dialog::prompt("修改候选译文", &message, current, "保存译文", done);
}

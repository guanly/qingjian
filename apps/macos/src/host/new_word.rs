//! 造词框：输入一个中文词，读音与形码编码都由引擎算好。
//!
//! 框本身是 [`crate::dialog`] 的非模态窗口（**不能用 `NSAlert.runModal`**：模态 run loop 会卡住
//! IMK 的按键派发，框里收不到字，别的应用也一起打不了），结果走回调，调用方再拿回 `Host` 存词。
//!
//! 只收中文：拼音逐字从词库反查、五笔编码按当前形码方案的词码规则拼——用户不必再手打音节，
//! 也就不会出现「字数和音节数对不上」那类错误。

use crate::dialog;

/// 造词框的说明：框里走的是青简自己，拼音 / 五笔都能打，候选窗照常出。
const MESSAGE: &str = "只填中文词，拼音会自动算好；选了形码方案时连编码一起算，两种打法都能出来。\n用拼音或五笔打出这个词，空格或数字选词，再按回车保存。";

/// 打开造词框，`prefill` 预填「最近上屏的那一段」；确定且非空时回调拿到词，取消 / 关窗拿到 `None`。
pub fn ask(prefill: &str, done: impl FnOnce(Option<String>) + 'static) {
    dialog::prompt("造词", MESSAGE, prefill, "存为用户词", done);
}

/// 造词失败的原因再给一个框说清楚：notice 行只在候选窗可见，菜单操作时窗口可能根本没开。
pub fn report(reason: &str) {
    dialog::notice("没有存上", reason);
}

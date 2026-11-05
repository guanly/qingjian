//! 造词框：一行一个中文词，读音与形码编码都由引擎算好。
//!
//! 框本身是 [`crate::dialog`] 的非模态窗口（**不能用 `NSAlert.runModal`**：模态 run loop 会卡住
//! IMK 的按键派发，框里收不到字，别的应用也一起打不了），结果走回调，调用方再拿回 `Host` 存词。
//!
//! 只收中文：拼音逐字从词库反查、五笔编码按当前形码方案的词码规则拼——用户不必再手打音节，
//! 也就不会出现「字数和音节数对不上」那类错误。

use crate::dialog;

/// 造词框的说明：框里走的是青简自己，拼音 / 五笔都能打，候选窗照常出。
const MESSAGE: &str =
    "每行一个词，只填中文：拼音与五笔编码都会自动算好。\n回车换行，⌘↩ 或点「存为用户词」保存。";

/// 打开造词框，`prefill` 预填「最近上屏的那一段」。
///
/// 确定且至少有一个词时回调拿到这些词（已切行、去空行、去重）；取消 / 关窗 / 全空给空列表。
pub fn ask(prefill: &str, done: impl FnOnce(Vec<String>) + 'static) {
    dialog::prompt_area("造词", MESSAGE, prefill, "存为用户词", move |text| {
        done(text.map(|text| words(&text)).unwrap_or_default());
    });
}

/// 一行一个词：切行、去首尾空白、丢掉空行与重复（同一个词留第一次出现的那个）。
fn words(text: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    for line in text.lines() {
        let word = line.trim();
        if word.is_empty() || words.iter().any(|seen| seen == word) {
            continue;
        }
        words.push(word.to_owned());
    }
    words
}

/// 造词失败的原因再给一个框说清楚：notice 行只在候选窗可见，菜单操作时窗口可能根本没开。
pub fn report(reason: &str) {
    dialog::notice("没有存上", reason);
}

#[cfg(test)]
mod tests {
    use super::words;

    #[test]
    fn one_word_per_line() {
        assert_eq!(words("开源\n青简\n"), ["开源", "青简"]);
    }

    #[test]
    fn blank_lines_and_spaces_are_dropped() {
        assert_eq!(words("  开源 \n\n\t\n青简  \n"), ["开源", "青简"]);
        assert!(words(" \n\n").is_empty());
    }

    #[test]
    fn duplicates_keep_the_first_one() {
        assert_eq!(words("开源\n青简\n开源"), ["开源", "青简"]);
    }

    #[test]
    fn windows_line_endings_work() {
        assert_eq!(words("开源\r\n青简"), ["开源", "青简"]);
    }
}

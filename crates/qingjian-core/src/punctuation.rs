//! 中文模式下的全角标点。
//!
//! 组句期间 `,` `.` 是翻页键、`-` `=` 也是，这些由壳决定；这里只回答「这个半角字符在中文模式下该变成什么」。

/// 这个半角字符是不是会转成全角标点的一员（`,` `.` `?` `!` 引号、括号、`/` 这些）。
/// 数字后的 `.` 保持半角是 [`Punctuation::convert`] 的状态决定的，这里照算「会转」，
/// 上屏候选后交给它判断即可。组句里到底要不要先上屏候选，看 [`commits`]。
pub fn converts(c: char) -> bool {
    matches!(
        c,
        ',' | '.'
            | '/'
            | '?'
            | '!'
            | ':'
            | ';'
            | '('
            | ')'
            | '['
            | ']'
            | '{'
            | '}'
            | '<'
            | '>'
            | '\\'
            | '^'
            | '_'
            | '$'
            | '~'
            | '"'
            | '\''
            | '`'
    )
}

/// 组句里敲这个字符要不要先上屏高亮候选、再补标点（`nihao,` 一气打完「你好，」）；
/// 不然它进缓冲区、整段成为英文直输段（`a@b`）。
///
/// 一般跟着 `[general] punct_commits` 开关走。`/` 是例外：中文模式下它就是顿号，与搜狗、微软拼音一致，
/// 敲它的人不会想打半角斜杠，所以不看开关。要半角 `/` 就切英文模式，或已经在直输段里接着敲（`no-way/x`）。
pub fn commits(c: char, punct_commits: bool) -> bool {
    converts(c) && (punct_commits || c == '/')
}

/// 引号成对切换的状态。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Punctuation {
    /// 下一个 `"` 是左引号。
    double_quote_open: bool,

    /// 下一个 `'` 是左引号。
    single_quote_open: bool,

    /// 上一个上屏字符是 ASCII 数字：`3.14` 里的 `.` 保持半角。
    after_digit: bool,
}

impl Punctuation {
    /// 半角字符对应的全角标点；不需要转换的返回 `None`，壳把原字符交给应用。
    pub fn convert(&mut self, c: char) -> Option<&'static str> {
        let after_digit = std::mem::replace(&mut self.after_digit, false);
        let converted = match c {
            ',' => "，",
            '.' if after_digit => return None,
            '.' => "。",
            '?' => "？",
            '!' => "！",
            ':' => "：",
            ';' => "；",
            '(' => "（",
            ')' => "）",
            '[' => "【",
            ']' => "】",
            // macOS / Windows 键盘 ⇧[ ⇧] 产生的字符；与 [ ] 的方头括号区分，出直角引号
            '{' => "「",
            '}' => "」",
            '<' => "《",
            '>' => "》",
            // 顿号：`/` 与 `\` 都给。`/` 是搜狗、微软拼音的习惯，手不用离开主键区；`\` 是 Windows 上的老习惯
            '/' | '\\' => "、",
            '^' => "……",
            '_' => "——",
            '$' => "￥",
            '~' => "～",
            // 外国人名的间隔号（约翰·史密斯），与微软拼音、搜狗一致（issue #831）
            '`' => "·",
            '"' => {
                self.double_quote_open = !self.double_quote_open;
                if self.double_quote_open { "“" } else { "”" }
            }
            '\'' => {
                self.single_quote_open = !self.single_quote_open;
                if self.single_quote_open { "‘" } else { "’" }
            }
            _ => return None,
        };
        Some(converted)
    }

    /// 壳把没有转换的字符原样交给应用后调用，用来记住「刚打了数字」。
    pub fn note_passthrough(&mut self, c: char) {
        self.after_digit = c.is_ascii_digit();
    }

    /// 有文本上屏后调用（候选、拼音、英文词），数字状态按最后一个字符更新。
    pub fn note_committed(&mut self, text: &str) {
        self.after_digit = text.chars().last().is_some_and(|c| c.is_ascii_digit());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_common_marks_and_toggles_quotes() {
        let mut p = Punctuation::default();
        assert_eq!(p.convert(','), Some("，"));
        assert_eq!(p.convert('"'), Some("“"));
        assert_eq!(p.convert('"'), Some("”"));
        assert_eq!(p.convert('\''), Some("‘"));
        assert_eq!(p.convert('\''), Some("’"));
        assert_eq!(p.convert('a'), None);
        assert_eq!(p.convert('-'), None);
    }

    // macOS 壳把 ⇧[ / ⇧] 当作字符 { / } 传进来，没有映射就原样上屏了半角花括号（issue #228）
    #[test]
    fn shifted_brackets_map_to_corner_quotes() {
        let mut p = Punctuation::default();
        assert_eq!(p.convert('{'), Some("「"));
        assert_eq!(p.convert('}'), Some("」"));
        assert_eq!(p.convert('['), Some("【"));
        assert_eq!(p.convert(']'), Some("】"));
    }

    #[test]
    fn backtick_maps_to_middle_dot() {
        assert_eq!(Punctuation::default().convert('`'), Some("·"));
    }

    /// 中文模式下 `/` 出顿号（搜狗、微软拼音同样如此），`\` 那份映射保留。
    #[test]
    fn slash_and_backslash_map_to_enumeration_comma() {
        assert_eq!(Punctuation::default().convert('/'), Some("、"));
        assert_eq!(Punctuation::default().convert('\\'), Some("、"));
        assert!(converts('/'));
    }

    /// 组句中标点先上屏候选跟着开关走，唯独 `/`（顿号）不看开关；不会转全角的符号永不先上屏。
    #[test]
    fn slash_commits_regardless_of_the_switch() {
        assert!(commits('/', false));
        assert!(commits('/', true));
        assert!(!commits(',', false));
        assert!(commits(',', true));
        assert!(!commits('@', true));
        assert!(!commits('-', true));
    }

    #[test]
    fn period_after_digit_stays_ascii() {
        let mut p = Punctuation::default();
        p.note_passthrough('3');
        assert_eq!(p.convert('.'), None);
        assert_eq!(p.convert('.'), Some("。"));
        p.note_committed("第1");
        assert_eq!(p.convert('.'), None);
        p.note_committed("开发");
        assert_eq!(p.convert('.'), Some("。"));
    }

    /// `converts` 与 `convert` 能转的字符集合一致（`after_digit` 的 `.` 除外，那是状态决定的）。
    #[test]
    fn converts_covers_exactly_the_convertible_characters() {
        for c in ('\t'..='~').filter(char::is_ascii_graphic) {
            let convertible = Punctuation::default().convert(c).is_some();
            assert_eq!(converts(c), convertible, "{c}");
        }
        assert!(converts('\''));
        assert!(converts('/'));
        assert!(!converts('a'));
        assert!(!converts('-'));
        assert!(!converts('@'));
        assert!(!converts('中'));
    }
}

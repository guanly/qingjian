use objc2_foundation::NSInteger;
use qingjian_core::FuzzyRules;

/// 模糊音条目的 tag 起点，后面加规则在 [`FuzzyRules::NAMES`] 里的下标。
const FUZZY_TAG_BASE: NSInteger = 100;

/// 菜单能触发的动作。编码进 NSMenuItem 的 tag，派发时再解出来。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    /// 开关云联想（写 `[predict] enabled`）。
    ToggleCloud,

    /// 开关一条模糊音规则，值是 [`FuzzyRules::NAMES`] 的下标。
    ToggleFuzzy(usize),

    /// 打开设置程序（装在 `Qingjian.app/Contents/Applications/` 里的 `QingjianSettings.app`）。
    OpenSettings,

    /// 打开旧版偏好设置窗口（留一个版本作回退）。
    OpenPreferences,

    /// 在访达里打开日志目录。
    OpenLogs,

    /// 在访达里打开配置目录（`~/Library/Application Support/Qingjian/`）：`config.toml`、
    /// 用户词、导入的词库与码表、主题都在这里。
    OpenConfigDir,

    /// 打开下载页（菜单里「有新版本」那一行）。
    OpenDownload,

    /// 在光标处打开符号面板（控制器处理，要当前会话的文本客户端）。
    OpenSymbolPanel,

    /// 造词：把刚才上屏的一段文字连同它的读音记成用户词。
    NewWord,
}

impl MenuAction {
    pub fn tag(self) -> NSInteger {
        match self {
            Self::ToggleCloud => 1,
            Self::OpenPreferences => 2,
            Self::OpenLogs => 3,
            Self::OpenDownload => 4,
            Self::OpenSettings => 5,
            Self::OpenSymbolPanel => 6,
            Self::NewWord => 7,
            Self::OpenConfigDir => 8,
            Self::ToggleFuzzy(index) => FUZZY_TAG_BASE + index as NSInteger,
        }
    }

    pub fn from_tag(tag: NSInteger) -> Option<Self> {
        Some(match tag {
            1 => Self::ToggleCloud,
            2 => Self::OpenPreferences,
            3 => Self::OpenLogs,
            4 => Self::OpenDownload,
            5 => Self::OpenSettings,
            6 => Self::OpenSymbolPanel,
            7 => Self::NewWord,
            8 => Self::OpenConfigDir,
            _ => {
                let index = usize::try_from(tag.checked_sub(FUZZY_TAG_BASE)?).ok()?;
                (index < FuzzyRules::NAMES.len()).then_some(Self::ToggleFuzzy(index))?
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_round_trip() {
        let all = [
            MenuAction::ToggleCloud,
            MenuAction::OpenSettings,
            MenuAction::OpenPreferences,
            MenuAction::OpenLogs,
            MenuAction::OpenDownload,
            MenuAction::OpenSymbolPanel,
            MenuAction::NewWord,
            MenuAction::OpenConfigDir,
            MenuAction::ToggleFuzzy(0),
            MenuAction::ToggleFuzzy(FuzzyRules::NAMES.len() - 1),
        ];
        for action in all {
            assert_eq!(MenuAction::from_tag(action.tag()), Some(action));
        }
        assert_eq!(MenuAction::from_tag(0), None);
        assert_eq!(
            MenuAction::from_tag(FUZZY_TAG_BASE + FuzzyRules::NAMES.len() as NSInteger),
            None
        );
        assert_eq!(MenuAction::from_tag(-1), None);
    }
}

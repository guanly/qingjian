use serde::{Deserialize, Serialize};

/// 配置文件 `[ranking]` 分节：候选排序里可以手工调整的部分。
///
/// 只管排序的输入，不开关功能；缺省全关。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RankingConfig {
    /// 句首不计入 `<s>`：语料里大量出现在句首的词（条目名、页面标题、页脚）句首概率被抬得比同音词高，
    /// 打开后所有词在句首只按词频算，句中照常走 bigram / trigram。缺省关。
    ///
    /// 只影响语料语言模型；个人 n-gram 里记的句首次数是用户自己打出来的，不跟着改。
    pub ignore_sentence_start: bool,
}

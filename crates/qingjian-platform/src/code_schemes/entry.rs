//! 找到的一套形码方案：还没读码表，只有名字、码表路径与规则。

use std::path::PathBuf;

use qingjian_core::CodeScheme;
use qingjian_dictionary::{AuxWordRules, CodeTable, DictionaryError};

use super::WUBI86;
use crate::{CodeRules, SchemeMeta};

/// 一套可用的形码方案。列表与设置页只要它；真要用时 [`Self::load`] 才读码表。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeSchemeEntry {
    /// 配置里 `[general] code` 写的值，也是用户目录下的目录名。
    pub id: String,

    pub label: String,

    pub meta: SchemeMeta,

    pub table: PathBuf,

    /// 用户导入的、给这套方案的形码词库（见 [`super::list_words`]），载入时并进码表。
    pub words: Vec<PathBuf>,

    pub auto_commit: bool,

    pub top: bool,

    /// `scheme.toml` 写了满码长度就是它，没写为 `None`（读码表时按最常见的编码长度算）。
    pub code_length: Option<usize>,

    /// 是不是随包的那套（设置页不给删）。用户目录里的同名方案盖过它时为 `false`。
    pub bundled: bool,
}

impl CodeSchemeEntry {
    /// 用户在设置里改过的规则盖上去（见 [`CodeRules`]）。
    pub fn with_rules(mut self, rules: CodeRules) -> Self {
        self.auto_commit = rules.auto_commit.unwrap_or(self.auto_commit);
        self.top = rules.top.unwrap_or(self.top);
        self
    }

    /// 读码表并上用户的形码词库，装成 Core 的方案。词库坏了只记日志、跳过：一本词库不能拖垮整套方案。
    pub fn load(&self) -> Result<CodeScheme, DictionaryError> {
        let mut table = CodeTable::from_path(&self.table)?;
        for path in &self.words {
            match CodeTable::from_path(path) {
                Ok(words) => table.merge(words),
                Err(error) => {
                    tracing::warn!(file = %path.display(), %error, "形码词库读不了，跳过");
                }
            }
        }
        let mut scheme = CodeScheme::new(self.log_key(), table);
        scheme.auto_commit = self.auto_commit;
        scheme.top = self.top;
        if let Some(length) = self.code_length.filter(|length| *length > 0) {
            scheme.code_length = length;
        }
        // 造词要给码表里没有的词算编码：86 五笔的词码规则是固定的（两字各前两码、三字各首码、
        // 四字以上取前三字与末字的首码），内置方案自己带上，不必让 `scheme.toml` 再写一遍。
        if self.id == WUBI86 {
            scheme.words = wubi_word_rules();
        }
        Ok(scheme)
    }

    /// 输入日志里的方案键：86 五笔沿用老日志的 `wubi`，其余就是方案 id。
    pub fn log_key(&self) -> String {
        if self.id == WUBI86 {
            "wubi".to_owned()
        } else {
            self.id.clone()
        }
    }
}

/// 86 五笔的词码规则：两字词各取前两码，三字词各取首码，四字及以上取前三字与末字的首码。
///
/// 公式里大写选字、小写选码位，两两成对：`AaAb` 是第一个字的头两个码，所以两字词要四对。
fn wubi_word_rules() -> AuxWordRules {
    AuxWordRules::parse([(2, "AaAbBaBb"), (3, "AaBaCa"), (4, "AaBaCaZa")]).unwrap_or_default()
}

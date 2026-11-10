//! 附加词库的装配：随包领域词库 + 用户 `dicts/` 下导入的词库。

use super::*;

impl Host {
    /// 按当前配置重新加载 `dicts/` 目录（设置程序导入、移除、开关之后靠文件快照发现并重装）。
    pub fn reload_dictionaries(&mut self) {
        let config = self.settings.config().dictionaries.clone();
        let loaded = extra_dictionaries::load(
            paths::bundled_dicts_dir().as_deref(),
            paths::dicts_dir().as_deref(),
            &config,
        );
        tracing::info!(count = loaded.len(), "附加词库已装配");
        self.engine.set_extra_dictionaries(loaded);
        self.applied_dictionaries = config;
        self.dictionary_files = dictionary_files();
    }
}

/// 用户 `dicts/` 的快照，只 stat。
pub(super) fn dictionary_files() -> Vec<(std::path::PathBuf, Option<std::time::SystemTime>, u64)> {
    paths::dicts_dir()
        .map(|dir| extra_dictionaries::snapshot(&dir))
        .unwrap_or_default()
}

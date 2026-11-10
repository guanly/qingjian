//! 输入日志的开关，以及打开外部程序。

use super::*;

impl Host {
    /// 输入日志文件的路径（数据目录下 `input-log.jsonl`）。
    pub fn input_log_path() -> Option<PathBuf> {
        paths::user_data_dir().map(|dir| dir.join("input-log.jsonl"))
    }

    /// 按开关接上 / 拆掉输入日志。
    pub(super) fn open_input_log(&mut self, enabled: bool) {
        match Self::input_log_path().filter(|_| enabled) {
            Some(path) => {
                tracing::info!(path = %path.display(), "输入日志开着");
                self.engine.set_input_logger(Box::new(InputLog::open(path)));
                let version = self.version.clone();
                self.engine.log_session(&version, "macos");
            }
            None => self.engine.set_input_logger(Box::new(NoInputLogger)),
        }
    }
}

pub(super) fn open_with_system(args: &[&str]) {
    if let Err(error) = std::process::Command::new("open").args(args).spawn() {
        tracing::warn!(%error, ?args, "open 失败");
    }
}

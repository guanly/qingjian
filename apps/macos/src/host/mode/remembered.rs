//! 按应用记住的中英模式（配置 `[apps] remember_mode`）：`~/Library/Application Support/Qingjian/app-modes.json`。
//!
//! 文件只存「应用 → 上次用的模式」，`true` 英文、`false` 中文。键按小写（bundle identifier 不区分大小写），
//! 与配置里的 `*` 前缀匹配无关：记录落在具体某个应用上，配置决定哪些应用要记。
//!
//! 写失败只记日志：记住模式是锦上添花，不能影响输入。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// 文件名，放在用户数据目录下。
const APP_MODES_FILE: &str = "app-modes.json";

/// 各应用上次用的中英模式。
pub struct AppModes {
    /// 落盘位置；拿不到用户数据目录时为 `None`，只在内存里记。
    path: Option<PathBuf>,

    /// 应用（小写）→ 英文模式。
    modes: BTreeMap<String, bool>,
}

impl AppModes {
    /// 用户数据目录里的 `app-modes.json`；目录拿不到就只在内存里记。
    pub fn load_in(dir: Option<PathBuf>) -> Self {
        Self::load(dir.map(|dir| dir.join(APP_MODES_FILE)))
    }

    /// 从磁盘读；没有文件、文件坏了都当没记过。
    pub fn load(path: Option<PathBuf>) -> Self {
        let modes = path.as_deref().and_then(read).unwrap_or_default();
        Self { path, modes }
    }

    /// 这个应用上次用的模式；没记过为 `None`。
    pub fn get(&self, app: &str) -> Option<bool> {
        self.modes.get(&normalize(app)).copied()
    }

    /// 记下这个应用现在的模式；与记录相同就不碰文件。
    pub fn set(&mut self, app: &str, english: bool) {
        let app = normalize(app);
        if self.modes.get(&app) == Some(&english) {
            return;
        }
        self.modes.insert(app, english);
        let Some(path) = &self.path else {
            return;
        };
        if let Err(error) = write(path, &self.modes) {
            tracing::warn!(%error, "failed to save per-app modes");
        }
    }
}

fn normalize(app: &str) -> String {
    app.to_lowercase()
}

fn read(path: &Path) -> Option<BTreeMap<String, bool>> {
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            tracing::warn!(%error, "failed to read per-app modes");
            return None;
        }
    };
    match serde_json::from_str(&source) {
        Ok(modes) => Some(modes),
        Err(error) => {
            // 手改坏了就重来一遍，别让输入法每次启动都读失败
            tracing::warn!(%error, "per-app modes file is not valid JSON, starting over");
            None
        }
    }
}

/// 先写临时文件再改名：进程中途被杀不会留下半个文件。
fn write(path: &Path, modes: &BTreeMap<String, bool>) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string_pretty(modes).map_err(std::io::Error::other)?;
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, json)?;
    std::fs::rename(&temp, path)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{AppModes, write};

    /// 每个用例一个独立文件：`process::id()` 保证并行跑的测试之间不撞车。
    fn temp_path(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "qingjian-app-modes-{}-{tag}.json",
            std::process::id()
        ))
    }

    #[test]
    fn modes_survive_a_reload() {
        let path = temp_path("reload");
        let _ = std::fs::remove_file(&path);
        let mut modes = AppModes::load(Some(path.clone()));
        assert_eq!(modes.get("com.apple.Terminal"), None);
        modes.set("com.apple.Terminal", true);
        modes.set("com.tencent.xinWeChat", false);

        let reloaded = AppModes::load(Some(path.clone()));
        assert_eq!(reloaded.get("com.apple.Terminal"), Some(true));
        assert_eq!(reloaded.get("com.tencent.xinWeChat"), Some(false));
        // bundle identifier 不区分大小写：切应用时报上来的大小写可能不同
        assert_eq!(reloaded.get("COM.APPLE.TERMINAL"), Some(true));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn recording_the_same_mode_does_not_rewrite_the_file() {
        let path = temp_path("noop");
        let mut modes = AppModes::load(Some(path.clone()));
        modes.set("com.apple.Terminal", false);
        let stamp = std::fs::metadata(&path)
            .and_then(|meta| meta.modified())
            .ok();
        modes.set("com.apple.Terminal", false);
        assert_eq!(
            std::fs::metadata(&path)
                .and_then(|meta| meta.modified())
                .ok(),
            stamp,
            "模式没变不该重写文件"
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_broken_file_starts_over_instead_of_failing() {
        let path = temp_path("broken");
        std::fs::write(&path, "{ not json").unwrap();
        let mut modes = AppModes::load(Some(path.clone()));
        assert_eq!(modes.get("com.apple.Terminal"), None);
        // 坏文件之后照常记新的
        modes.set("com.apple.Terminal", true);
        assert_eq!(
            AppModes::load(Some(path.clone())).get("com.apple.Terminal"),
            Some(true)
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn no_user_directory_keeps_modes_in_memory() {
        let mut modes = AppModes::load(None);
        modes.set("com.apple.Terminal", true);
        assert_eq!(modes.get("com.apple.Terminal"), Some(true));
    }

    #[test]
    fn the_file_is_readable_json() {
        let path = temp_path("format");
        let mut modes = BTreeMap::new();
        modes.insert("com.apple.Terminal".to_owned(), true);
        write(&path, &modes).unwrap();
        let source = std::fs::read_to_string(&path).unwrap();
        assert_eq!(source.trim(), "{\n  \"com.apple.Terminal\": true\n}");
        let _ = std::fs::remove_file(path);
    }
}

//! 按应用的设置：选一个 `.app`，从它的 `Info.plist` 认 bundle identifier 与名字。
//!
//! 配置里存的是 bundle identifier（应用自己报给输入法的就是它，切应用时才对得上），界面上显示名字：
//! 标准应用目录里扫一遍，扫不到就照原样显示 identifier。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use super::Settings;
use crate::i18n::{tr, tr_args};

impl Settings {
    /// 「记住中英模式的应用」：选一个应用加进名单；本来就在名单里就只提示一句。
    pub(super) fn remember_app(&mut self) {
        let Some(path) = pick_app() else {
            return;
        };
        let Some((bundle, name)) = read_info(&path) else {
            self.notice = Some(("remember-app", tr("language.remember_no_bundle").to_owned()));
            return;
        };
        let mut apps = self.config.apps.remember_mode.clone();
        if !apps.iter().any(|app| app.eq_ignore_ascii_case(&bundle)) {
            apps.push(bundle);
            self.write_list("apps", "remember_mode", &apps);
        }
        self.notice = Some((
            "remember-app",
            tr_args("language.remember_added", &[("app", &name)]),
        ));
    }
}

/// 选一个程序：macOS 是 `.app`（其实是目录，打开面板把它当一个文件选），Windows 是 `.exe`。
fn pick_app() -> Option<PathBuf> {
    let (extension, start) = if cfg!(target_os = "macos") {
        ("app", "/Applications")
    } else {
        ("exe", "")
    };
    let mut dialog = rfd::FileDialog::new()
        .set_title(tr("language.remember_pick_app"))
        .add_filter(tr("language.remember_apps"), &[extension]);
    if !start.is_empty() {
        dialog = dialog.set_directory(start);
    }
    dialog.pick_file()
}

/// 界面上显示的名字：标准应用目录里能扫到就用应用自己的名字，扫不到就照原样显示 bundle identifier。
pub(crate) fn label(bundle: &str) -> String {
    installed()
        .get(bundle)
        .cloned()
        .unwrap_or_else(|| bundle.to_owned())
}

/// 界面上显示的名字，以及需要额外标出标识时的那一行说明。
///
/// 两个应用可能同名：`IntelliJ IDEA` 与 `IntelliJ IDEA CE` 的 `CFBundleName` 都是 “IntelliJ IDEA”，
/// 名单里只写名字就分不出加的是哪一个（加错了那一行永远不生效，看着就是「记住模式没起作用」）。
/// 名字撞车时把 bundle identifier 露在名字下面，名字唯一的照旧只显示名字。
pub(crate) fn label_with_note(bundle: &str) -> (String, Option<String>) {
    let name = label(bundle);
    let note = shared_name(bundle, &name, installed()).then(|| bundle.to_owned());
    (name, note)
}

/// 除了 `bundle` 自己，还有没有别的已安装应用叫这个名字。
fn shared_name(bundle: &str, name: &str, installed: &HashMap<String, String>) -> bool {
    installed
        .iter()
        .any(|(other, other_name)| !other.eq_ignore_ascii_case(bundle) && other_name == name)
}

/// 本机应用的 bundle identifier → 名字（每个进程扫一次，几十毫秒）。
fn installed() -> &'static HashMap<String, String> {
    static INSTALLED: OnceLock<HashMap<String, String>> = OnceLock::new();
    INSTALLED.get_or_init(|| {
        let mut apps = HashMap::new();
        for dir in app_dirs() {
            scan(&dir, 1, &mut apps);
        }
        apps
    })
}

/// 系统与应用目录：`/Applications` 里偶尔还有一层子目录（`Utilities`、各家安装器的分组），往下多走一层。
fn app_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![
        PathBuf::from("/Applications"),
        PathBuf::from("/System/Applications"),
        PathBuf::from("/System/Applications/Utilities"),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join("Applications"));
    }
    dirs
}

fn scan(dir: &Path, depth: usize, apps: &mut HashMap<String, String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|extension| extension == "app") {
            if let Some((bundle, name)) = read_info(&path) {
                apps.entry(bundle).or_insert(name);
            }
        } else if depth > 0 && path.is_dir() {
            scan(&path, depth - 1, apps);
        }
    }
}

/// `.app` 的 bundle identifier 与名字；`Info.plist` 坏了、没写 identifier 都不认。
fn read_info(app: &Path) -> Option<(String, String)> {
    let info = plist::Value::from_file(app.join("Contents/Info.plist")).ok()?;
    let info = info.as_dictionary()?;
    let bundle = info.get("CFBundleIdentifier")?.as_string()?.to_owned();
    let name = info
        .get("CFBundleDisplayName")
        .or_else(|| info.get("CFBundleName"))
        .and_then(plist::Value::as_string)
        .map(str::to_owned)
        .or_else(|| {
            app.file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
        })?;
    Some((bundle, name))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    /// 系统自带的终端：scan 扫得到，名字是它的显示名而不是路径。
    #[test]
    #[cfg(target_os = "macos")]
    fn the_system_terminal_is_found_by_bundle_identifier() {
        let path = Path::new("/System/Applications/Utilities/Terminal.app");
        assert_eq!(
            read_info(path).map(|(bundle, _)| bundle).as_deref(),
            Some("com.apple.Terminal")
        );
        assert_eq!(label("com.apple.Terminal"), "Terminal");
        // 扫不到的照原样显示，不报错
        assert_eq!(
            label("com.example.NotInstalled"),
            "com.example.NotInstalled"
        );
    }

    /// 两个 IntelliJ 的 `CFBundleName` 都是 “IntelliJ IDEA”：这种时候名字下面要带出标识。
    fn intellij_installs() -> HashMap<String, String> {
        HashMap::from([
            (
                "com.jetbrains.intellij".to_owned(),
                "IntelliJ IDEA".to_owned(),
            ),
            (
                "com.jetbrains.intellij.ce".to_owned(),
                "IntelliJ IDEA".to_owned(),
            ),
            ("com.apple.Terminal".to_owned(), "Terminal".to_owned()),
        ])
    }

    #[test]
    fn a_shared_display_name_gets_the_bundle_identifier_below_it() {
        let installed = intellij_installs();
        assert!(
            shared_name("com.jetbrains.intellij.ce", "IntelliJ IDEA", &installed),
            "同名 → 要标出标识"
        );
        assert!(shared_name(
            "com.jetbrains.intellij",
            "IntelliJ IDEA",
            &installed
        ));
        assert!(
            !shared_name("com.apple.Terminal", "Terminal", &installed),
            "名字唯一就不啰嗦"
        );
        assert!(
            !shared_name("com.example.Gone", "com.example.Gone", &installed),
            "没装的应用照原样显示标识，不用再标一遍"
        );
    }
}

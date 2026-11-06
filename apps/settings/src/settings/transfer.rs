//! 「高级」页的设置导入导出：设置与用户造的词（造词框造的音码词、`code_dicts/` 下的形码词）
//! 一起打成一个 zip（规则在 `qingjian_platform::export_settings` / `import_settings` 与两个
//! `merge_user_words`）。导入前把现在的配置另存一份。

use std::path::{Path, PathBuf};
use std::process::Command;

use qingjian_platform::merge_user_words;
use qingjian_platform::{USER_WORDS_FILE, code_schemes, export_settings, import_settings};

use super::Settings;

use crate::i18n::{tr, tr_args};
use crate::system::paths;

/// 消息挂在哪一行下面。
const SLOT: &str = "config-transfer";

/// 导入前现在的配置另存成这个名字（与配置同目录），导错了能拷回来。
const BEFORE_IMPORT: &str = "config.before-import.toml";

/// zip 里的设置文件名；用户词沿用数据目录里的名字（`user-words.tsv` 与 `code_dicts/user-words.tsv`）。
const CONFIG_FILE: &str = "config.toml";

/// 形码自造词在数据目录里的子目录（与 `qingjian_platform::code_schemes` 一致）。
const CODE_WORDS_DIR: &str = "code_dicts";

impl Settings {
    /// 导出：设置（去掉短语、密钥、状态条位置）与本机的用户词装进一个 zip。
    pub(super) fn export_config(&mut self) {
        let source = match std::fs::read_to_string(&self.path) {
            Ok(source) => source,
            Err(error) => {
                self.notice = Some((
                    SLOT,
                    tr_args("transfer.cant_read_config", &[("error", &error)]),
                ));
                return;
            }
        };
        let exported = match export_settings(&source) {
            Ok(exported) => exported,
            Err(error) => {
                self.notice = Some((SLOT, error));
                return;
            }
        };
        let name = tr_args(
            "transfer.file_name",
            &[("date", &jiff::Zoned::now().date())],
        );
        let Some(target) = rfd::FileDialog::new()
            .set_title(tr("transfer.export_title"))
            .set_file_name(&name)
            .add_filter(tr("transfer.file_filter"), &["zip"])
            .save_file()
        else {
            return;
        };
        let target = with_zip_extension(target);
        let notice = match self.pack(&exported, &target) {
            Ok(true) => tr_args("transfer.exported_words", &[("path", &target.display())]),
            Ok(false) => tr_args("transfer.exported", &[("path", &target.display())]),
            Err(error) => error,
        };
        self.notice = Some((SLOT, notice));
    }

    /// 导入：zip 里的设置换掉现有的，用户词与本机的合并（同名同音以包内为准）。
    pub(super) fn import_config(&mut self) {
        let Some(source) = rfd::FileDialog::new()
            .set_title(tr("transfer.import_title"))
            .add_filter(tr("transfer.file_filter"), &["zip"])
            .add_filter(tr("import.all_files"), &["*"])
            .pick_file()
        else {
            return;
        };
        let current = std::fs::read_to_string(&self.path).unwrap_or_default();
        // 0.3.2 及以前导出的是单个 `.toml`：照旧只换设置，用户词那边不动
        let result = if looks_like_zip(&source) {
            self.restore(&source, &current)
        } else {
            self.restore_settings_only(&source, &current)
        };
        self.reload();
        self.notice = Some((SLOT, result.unwrap_or_else(|error| error)));
    }

    /// 旧版导出的 `.toml`：只换设置，没有用户词可合并。
    fn restore_settings_only(&self, source: &Path, current: &str) -> Result<String, String> {
        let imported = std::fs::read_to_string(source)
            .map_err(|error| tr_args("transfer.cant_read", &[("error", &error)]))?;
        let merged = import_settings(current, &imported)?;
        let backup = self.path.with_file_name(BEFORE_IMPORT);
        if !current.is_empty() {
            std::fs::write(&backup, current)
                .map_err(|error| tr_args("transfer.backup_failed", &[("error", &error)]))?;
        }
        std::fs::write(&self.path, merged)
            .map_err(|error| tr_args("transfer.cant_write", &[("error", &error)]))?;
        Ok(tr_args("transfer.imported", &[("file", &BEFORE_IMPORT)]))
    }

    /// 写临时目录、压成 zip；返回包里有没有带上用户词。
    fn pack(&self, config: &str, target: &Path) -> Result<bool, String> {
        let dir = scratch()?;
        let result = (|| {
            std::fs::write(dir.join(CONFIG_FILE), config)
                .map_err(|error| tr_args("transfer.cant_write", &[("error", &error)]))?;
            let mut names = vec![CONFIG_FILE.to_owned()];
            let mut words = false;
            // 只带这两个用户词文件：`code_dicts/` 里还可能有导入的五笔词库，那是词库不是造词，
            // 几十 MB 一起打包没有意义，导入方也不该被塞一份
            for relative in [
                USER_WORDS_FILE.to_owned(),
                format!("{CODE_WORDS_DIR}/{USER_WORDS_FILE}"),
            ] {
                let source = self.data_dir().join(&relative);
                if !source.is_file() {
                    continue;
                }
                let copy = dir.join(&relative);
                if let Some(parent) = copy.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|error| tr_args("transfer.cant_write", &[("error", &error)]))?;
                }
                std::fs::copy(&source, &copy)
                    .map_err(|error| tr_args("transfer.cant_write", &[("error", &error)]))?;
                names.push(relative);
                words = true;
            }
            zip_dir(&dir, target, &names)?;
            Ok(words)
        })();
        let _ = std::fs::remove_dir_all(&dir);
        result
    }

    /// 解开 zip、换设置、合并用户词。返回给用户看的那句话。
    fn restore(&self, source: &Path, current: &str) -> Result<String, String> {
        let dir = scratch()?;
        let result = (|| {
            unzip_into(source, &dir)?;
            // 别的工具重新压过的包会多一层文件夹，从设置文件所在的那层取其余文件
            let config = find(&dir, CONFIG_FILE, 0).ok_or_else(|| tr("transfer.not_a_bundle"))?;
            let base = config.parent().unwrap_or(&dir);
            let imported = std::fs::read_to_string(&config)
                .map_err(|error| tr_args("transfer.cant_read", &[("error", &error)]))?;
            let merged = import_settings(current, &imported)?;
            let backup = self.path.with_file_name(BEFORE_IMPORT);
            if !current.is_empty() {
                std::fs::write(&backup, current)
                    .map_err(|error| tr_args("transfer.backup_failed", &[("error", &error)]))?;
            }
            std::fs::write(&self.path, merged)
                .map_err(|error| tr_args("transfer.cant_write", &[("error", &error)]))?;
            let words = self.restore_words(base)?;
            Ok(if words {
                tr_args("transfer.imported_words", &[("file", &BEFORE_IMPORT)])
            } else {
                tr_args("transfer.imported", &[("file", &BEFORE_IMPORT)])
            })
        })();
        let _ = std::fs::remove_dir_all(&dir);
        result
    }

    /// 把包里的用户词并进本机那两份（音码与形码各一份）；返回有没有带用户词。
    fn restore_words(&self, base: &Path) -> Result<bool, String> {
        let data = self.data_dir();
        let mut words = false;
        for (target, merge) in [
            (
                data.join(USER_WORDS_FILE),
                merge_user_words as fn(&str, &str) -> String,
            ),
            (
                data.join(CODE_WORDS_DIR).join(USER_WORDS_FILE),
                code_schemes::merge_user_words,
            ),
        ] {
            let packed = match target.strip_prefix(&data) {
                Ok(relative) => base.join(relative),
                Err(_) => continue,
            };
            let Ok(imported) = std::fs::read_to_string(&packed) else {
                continue;
            };
            let local = std::fs::read_to_string(&target).unwrap_or_default();
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|error| tr_args("transfer.cant_write", &[("error", &error)]))?;
            }
            write_atomic_str(&target, &merge(&local, &imported))?;
            words = true;
        }
        Ok(words)
    }

    /// 用户词所在的目录。Linux 上配置在 `~/.config/qingjian/`、数据在 `~/.local/share/qingjian/`，
    /// 不是一个地方，所以按平台路径取，不拿配置文件的父目录凑。
    fn data_dir(&self) -> PathBuf {
        paths::user_dir().unwrap_or_else(|| {
            self.path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("."))
        })
    }
}

/// 名字里没写扩展名时补上 `.zip`：访达与资源管理器都靠它认这是什么文件。
fn with_zip_extension(mut target: PathBuf) -> PathBuf {
    if target.extension().is_none() {
        target.set_extension("zip");
    }
    target
}

/// 看文件头判断是不是 zip（`PK\x03\x04`）：扩展名不可靠，用户可能把旧包改名。
fn looks_like_zip(path: &Path) -> bool {
    use std::io::Read;

    let mut head = [0u8; 4];
    std::fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut head))
        .is_ok_and(|()| head == *b"PK\x03\x04")
}

/// 每次都换一个临时目录（导入导出可能同时开着两个窗口），用完删掉。
fn scratch() -> Result<PathBuf, String> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.subsec_nanos())
        .unwrap_or_default();
    let dir =
        std::env::temp_dir().join(format!("qingjian-transfer-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir)
        .map_err(|error| tr_args("transfer.cant_write", &[("error", &error)]))?;
    Ok(dir)
}

/// 把临时目录里列出的几项压成 zip。Windows 10 起自带 bsdtar（`-a` 按扩展名出 zip），
/// macOS 与 Linux 用 `zip`；两者都在目录里执行，包内路径保持 `code_dicts/user-words.tsv` 这样。
fn zip_dir(dir: &Path, target: &Path, names: &[String]) -> Result<(), String> {
    let _ = std::fs::remove_file(target);
    let status = if cfg!(windows) {
        Command::new("tar.exe")
            .arg("-a")
            .arg("-cf")
            .arg(target)
            .arg("-C")
            .arg(dir)
            .args(names)
            .status()
    } else {
        Command::new("zip")
            .arg("-qr")
            .arg(target)
            .args(names)
            .current_dir(dir)
            .status()
    }
    .map_err(|error| tr_args("transfer.cant_zip", &[("error", &error)]))?;
    if !status.success() {
        return Err(tr_args("transfer.zip_failed", &[("status", &status)]));
    }
    Ok(())
}

/// 解开 zip 到临时目录。
fn unzip_into(source: &Path, dir: &Path) -> Result<(), String> {
    let status = if cfg!(windows) {
        Command::new("tar.exe")
            .arg("-xf")
            .arg(source)
            .arg("-C")
            .arg(dir)
            .status()
    } else {
        Command::new("unzip")
            .arg("-oq")
            .arg(source)
            .arg("-d")
            .arg(dir)
            .status()
    }
    .map_err(|error| tr_args("transfer.cant_unzip", &[("error", &error)]))?;
    if !status.success() {
        return Err(tr_args("transfer.unzip_failed", &[("status", &status)]));
    }
    Ok(())
}

/// 在解开的目录里找包内文件。自己打的包在根上；用户用访达「压缩」过、多套了一层文件夹的也认。
/// 深度限 3 层，避免压缩包里塞了整棵目录树时在这里转圈。
fn find(root: &Path, name: &str, depth: usize) -> Option<PathBuf> {
    let direct = root.join(name);
    if direct.is_file() {
        return Some(direct);
    }
    if depth >= 3 {
        return None;
    }
    let children = std::fs::read_dir(root).ok()?;
    for child in children.flatten() {
        let path = child.path();
        if path.is_dir()
            && let Some(found) = find(&path, name, depth + 1)
        {
            return Some(found);
        }
    }
    None
}

/// 原子写：输入法正在跑，不能让它的读取看到写了一半的表。
fn write_atomic_str(path: &Path, text: &str) -> Result<(), String> {
    qingjian_core::storage::write_atomic_str(path, text)
        .map_err(|error| tr_args("transfer.cant_write", &[("error", &error)]))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{CONFIG_FILE, find, looks_like_zip, unzip_into, with_zip_extension, zip_dir};

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("qingjian-transfer-test-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 自己打的包解开后目录层级不变：`config.toml` 在根上，形码词在 `code_dicts/` 下面。
    /// Windows 走 `tar.exe`，只在 macOS 与 Linux 上验这条。
    #[cfg(not(windows))]
    #[test]
    fn zip_round_trip_keeps_the_layout() {
        let dir = temp("pack");
        std::fs::write(dir.join(CONFIG_FILE), "[general]\npage_size = 5\n").unwrap();
        std::fs::create_dir_all(dir.join("code_dicts")).unwrap();
        std::fs::write(dir.join("code_dicts/user-words.tsv"), "拉混\truji\t100\n").unwrap();
        let target = temp("pack-out").join("bundle.zip");
        zip_dir(
            &dir,
            &target,
            &[CONFIG_FILE.to_owned(), "code_dicts".to_owned()],
        )
        .unwrap();

        let out = temp("unpack");
        unzip_into(&target, &out).unwrap();
        let config = find(&out, CONFIG_FILE, 0).unwrap();
        assert_eq!(config.parent().unwrap(), out.as_path());
        assert!(out.join("code_dicts/user-words.tsv").is_file());
    }

    /// 别的工具重新压过、多套了一层文件夹的包也认。
    #[test]
    fn find_looks_into_a_wrapping_folder() {
        let root = temp("wrapped");
        std::fs::create_dir_all(root.join("青简设置")).unwrap();
        std::fs::write(root.join("青简设置").join(CONFIG_FILE), "").unwrap();
        assert!(find(&root, CONFIG_FILE, 0).is_some());
        assert!(find(&temp("nothing"), CONFIG_FILE, 0).is_none());
    }

    /// 用户删掉扩展名时补上，已经有了就不重复。
    #[test]
    fn zip_extension_is_added_once() {
        assert_eq!(
            with_zip_extension(PathBuf::from("/tmp/a")),
            PathBuf::from("/tmp/a.zip")
        );
        assert_eq!(
            with_zip_extension(PathBuf::from("/tmp/a.zip")),
            PathBuf::from("/tmp/a.zip")
        );
    }

    /// 旧版导出的是 `.toml`，不能被当成 zip 硬解；zip 才走带用户词的那条路。
    #[test]
    fn zip_is_recognized_by_its_header() {
        let dir = temp("header");
        let plain = dir.join("qingjian.toml");
        std::fs::write(&plain, "[general]\npage_size = 5\n").unwrap();
        assert!(!looks_like_zip(&plain));
        assert!(!looks_like_zip(&dir.join("missing.zip")));

        #[cfg(not(windows))]
        {
            let zip = dir.join("bundle.zip");
            std::fs::write(dir.join(CONFIG_FILE), "").unwrap();
            zip_dir(&dir, &zip, &[CONFIG_FILE.to_owned()]).unwrap();
            assert!(looks_like_zip(&zip));
        }
    }
}

//! 形码方案：随包的（如 86 五笔）与用户数据目录 `schemes/<id>/` 下自己放进去的，同一种格式、同一套找法。
//!
//! 一套方案一个目录，目录名就是 id：`scheme.toml` 写名字、元信息与按键规则（见 `Descriptor`，必须有），码表是 `词\t编码\t词频` 的 TSV。
//! 用户目录里与随包同 id 的方案盖过随包那套；老位置 `wubi/wubi86.tsv` 仍认，只换随包五笔的码表。
//! 用户导入的形码词库在用户目录 `code_dicts/` 下（见 [`qingjian_dictionary::import::import_code_words`]），按文件头记的方案 id
//! 跟那套方案的码表合在一起，不用整份换掉码表。
//! 随包的 `schemes` 目录在哪由壳给（macOS 在 `Resources/`，Windows 在安装目录的 `assets/`）。

mod descriptor;
mod entry;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use qingjian_dictionary::import::{SCHEME_HEADER, code_words_scheme};

use self::descriptor::Descriptor;
pub use self::entry::CodeSchemeEntry;
use crate::Scheme;

/// `scheme.toml` 里 `type` 写这个（或不写）就是形码方案。
pub const KIND: &str = "code";

/// 随包的 86 五笔。
pub const WUBI86: &str = "wubi86";

/// 方案目录名，随包与用户数据目录下都叫这个。
pub const DIR: &str = "schemes";

pub(crate) const DESCRIPTOR: &str = "scheme.toml";

/// 导入的形码词库的目录名，在用户数据目录下。
const WORDS_DIR: &str = "code_dicts";

/// 老位置：用户自己换的五笔码表（2026-09 起就认这里）。
const LEGACY_WUBI_TABLE: &str = "wubi/wubi86.tsv";

/// 能当方案 id 的写法：小写字母、数字、`-` `_`，而且不能与拼音侧的写法或日志里 86 五笔的 `wubi` 撞上——
/// 输入日志把两侧拼成 `xiaohe+<id>`，撞了回放就认错方案。`wubi86` 例外：它在拼音侧是老配置的写法，
/// 在这里就是随包五笔本身（用户目录里放 `schemes/wubi86/` 是换掉随包那套）。
pub fn is_valid_id(id: &str) -> bool {
    if id == WUBI86 {
        return true;
    }
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_'))
        && id != "wubi"
        && id.parse::<Scheme>().is_err()
}

/// 全部可用的方案：随包的在前，用户的在后，各自按 id 排；用户方案与随包同 id 时占随包那个位置。
/// `bundled` 是随包的 `schemes` 目录。
pub fn list(user_dir: Option<&Path>, bundled: Option<&Path>) -> Vec<CodeSchemeEntry> {
    let mut entries = bundled.map_or_else(Vec::new, |dir| scan(dir, true));
    if let Some(user_dir) = user_dir {
        let legacy = user_dir.join(LEGACY_WUBI_TABLE);
        if legacy.is_file()
            && let Some(wubi) = entries.iter_mut().find(|entry| entry.id == WUBI86)
        {
            wubi.table = legacy;
        }
        for entry in scan(&user_dir.join(DIR), false) {
            match entries.iter_mut().find(|bundled| bundled.id == entry.id) {
                Some(slot) => *slot = entry,
                None => entries.push(entry),
            }
        }
        for words in list_words(&user_words_dir(user_dir)) {
            if let Some(entry) = entries.iter_mut().find(|entry| entry.id == words.scheme) {
                entry.words.push(words.path);
            }
        }
    }
    entries
}

/// 用户导入的一本形码词库。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeWords {
    /// 文件名（不含扩展名），`[dictionaries] disabled` 按它开关。
    pub stem: String,

    pub path: PathBuf,

    /// 给哪套方案的。
    pub scheme: String,
}

/// 形码词库目录（`dir`，见 [`user_words_dir`]）的逐文件快照（路径、mtime、长度）：壳每秒比一次，设置页刚导入、移除就重装方案，不等配置改动。
pub fn words_snapshot(dir: &Path) -> Vec<(PathBuf, Option<SystemTime>, u64)> {
    word_files(dir)
        .into_iter()
        .filter_map(|path| {
            let metadata = std::fs::metadata(&path).ok()?;
            Some((path, metadata.modified().ok(), metadata.len()))
        })
        .collect()
}

/// 形码词库目录里的词库，按文件名排。
pub fn list_words(dir: &Path) -> Vec<CodeWords> {
    word_files(dir)
        .into_iter()
        .filter_map(|path| {
            Some(CodeWords {
                stem: path.file_stem()?.to_str()?.to_owned(),
                scheme: code_words_scheme(&path)?,
                path,
            })
        })
        .collect()
}

/// 目录里的 `.tsv`，按路径排；以 `.` 开头的（访达拷出来的 `._*`）跳过。
fn word_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(children) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = children
        .filter_map(Result::ok)
        .map(|child| child.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| !name.starts_with('.') && name.ends_with(".tsv"))
        })
        .collect();
    files.sort();
    files
}

/// 按 id 找一套方案；找不到为 `None`。
pub fn find(id: &str, user_dir: Option<&Path>, bundled: Option<&Path>) -> Option<CodeSchemeEntry> {
    list(user_dir, bundled)
        .into_iter()
        .find(|entry| entry.id == id)
}

/// 用户方案的目录：`<用户数据目录>/schemes`，不负责创建。
pub fn user_schemes_dir(user_dir: &Path) -> PathBuf {
    user_dir.join(DIR)
}

/// 导入的形码词库的目录：`<用户数据目录>/code_dicts`，不负责创建。
pub fn user_words_dir(user_dir: &Path) -> PathBuf {
    user_dir.join(WORDS_DIR)
}

/// 自造形码词落在哪本词库：`code_dicts/user-words.tsv`。与导入的词库同一套格式与热加载。
pub const USER_WORDS_FILE: &str = "user-words.tsv";

/// 把一条自造的形码词写进用户形码词库，返回文件路径。
///
/// 同一个词写过就换成新的编码（改正读音、换方案后重造都要生效）；文件头记方案 id，
/// 换了方案时旧词不再合并——编码对不上，留着只会串码。
/// 目录不存在会建，写文件失败由调用方决定要不要提示。
pub fn save_user_word(
    user_dir: &Path,
    scheme: &str,
    text: &str,
    code: &str,
) -> std::io::Result<PathBuf> {
    let dir = user_words_dir(user_dir);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(USER_WORDS_FILE);
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let mut kept: Vec<&str> = Vec::new();
    let mut same_scheme = false;
    for line in existing.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix(SCHEME_HEADER) {
            same_scheme = rest.trim() == scheme;
            continue;
        }
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.split('\t').next() == Some(text) {
            continue;
        }
        kept.push(trimmed);
    }
    // 文件里是别的方案的词：整份不要了，按当前方案重建
    if !same_scheme {
        kept.clear();
    }
    let mut out = format!("{SCHEME_HEADER} {scheme}\n");
    for line in kept {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str(&format!("{text}\t{code}\t100\n"));
    std::fs::write(&path, out)?;
    Ok(path)
}

/// 合并两份自造形码词（[`USER_WORDS_FILE`]）：按词去重，包内那条为准，本机独有的词留在后面。
///
/// 文件头的方案 id（[`SCHEME_HEADER`]）不一样就整份用 `imported`：编码与方案绑死，
/// 两套混在一起只会串码，与 [`save_user_word`] 换了方案时丢掉旧词的做法一致。
/// 有一边没写头（老文件）时只按词合并，不拿不准的信息去丢词。
pub fn merge_user_words(local: &str, imported: &str) -> String {
    if let (Some(here), Some(there)) = (scheme_of(local), scheme_of(imported))
        && here != there
    {
        return imported.to_owned();
    }
    let mut out = String::new();
    if let Some(scheme) = scheme_of(imported) {
        out.push_str(&format!("{SCHEME_HEADER} {scheme}\n"));
    }
    let mut seen: HashSet<String> = HashSet::new();
    for line in imported.lines().chain(local.lines()) {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let word = trimmed.split('\t').next().unwrap_or_default();
        if seen.insert(word.to_owned()) {
            out.push_str(trimmed);
            out.push('\n');
        }
    }
    out
}

/// 词库文件头记的方案 id。
fn scheme_of(text: &str) -> Option<&str> {
    text.lines()
        .find_map(|line| line.trim().strip_prefix(SCHEME_HEADER).map(str::trim))
}

/// 一个 `schemes` 目录下的方案，按 id 排。不能用的跳过并记日志：一套方案坏了不能拖垮别的。
fn scan(dir: &Path, bundled: bool) -> Vec<CodeSchemeEntry> {
    let Ok(children) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut entries: Vec<CodeSchemeEntry> = children
        .filter_map(Result::ok)
        .map(|child| child.path())
        .filter(|path| path.is_dir())
        .filter_map(|path| read(&path, bundled))
        .collect();
    entries.sort_by(|a, b| a.id.cmp(&b.id));
    entries
}

/// 读一个用户方案目录（目录名就是 id）；不能用时记日志、返回 `None`。
pub fn from_dir(dir: &Path) -> Option<CodeSchemeEntry> {
    read(dir, false)
}

fn read(dir: &Path, bundled: bool) -> Option<CodeSchemeEntry> {
    let id = dir.file_name()?.to_str()?.to_owned();
    if !is_valid_id(&id) {
        tracing::warn!(dir = %dir.display(), "形码方案的目录名不能当 id，跳过");
        return None;
    }
    let path = dir.join(DESCRIPTOR);
    let descriptor: Descriptor = match std::fs::read_to_string(&path) {
        Ok(text) => match toml::from_str(&text) {
            Ok(descriptor) => descriptor,
            Err(error) => {
                tracing::warn!(file = %path.display(), %error, "scheme.toml 解析失败，跳过");
                return None;
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            tracing::warn!(dir = %dir.display(), "方案目录里没有 scheme.toml，跳过");
            return None;
        }
        Err(error) => {
            tracing::warn!(file = %path.display(), %error, "scheme.toml 读不了，跳过");
            return None;
        }
    };
    match descriptor.kind.as_str() {
        "" | KIND => {}
        crate::shuangpin_schemes::KIND | crate::aux_schemes::KIND => return None,
        kind => {
            tracing::warn!(file = %path.display(), kind, "不认识的方案种类，跳过");
            return None;
        }
    }
    let table = dir.join(&descriptor.table);
    if !table.is_file() {
        tracing::warn!(file = %table.display(), "形码方案的码表不在，跳过");
        return None;
    }
    let label = match descriptor.name.trim() {
        "" => id.clone(),
        name => name.to_owned(),
    };
    Some(CodeSchemeEntry {
        id,
        label,
        meta: descriptor.meta,
        table,
        words: Vec::new(),
        auto_commit: descriptor.auto_commit,
        top: descriptor.top,
        code_length: descriptor.code_length,
        bundled,
    })
}

#[cfg(test)]
mod tests;

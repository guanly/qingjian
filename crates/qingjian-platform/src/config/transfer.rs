//! 设置的导出与导入（换电脑、备份用）：只搬设置与用户造的词，不带自定义短语（短语另有自己的导入导出）、
//! 云服务密钥与悬浮状态条在这台屏幕上的位置。导入前先当配置读一遍，读不通就不动现有文件。

use std::collections::HashSet;

use toml_edit::{DocumentMut, Item};

use super::Config;

use crate::i18n::tr_args;

/// 只属于这台电脑、导出时去掉、导入时保留本机原值的项（分节，键）；`custom_phrases` 是整张表。
const LOCAL: &[(&str, &str)] = &[
    ("predict", "api_key"),
    ("status_bar", "x"),
    ("status_bar", "y"),
];

/// 自定义短语的表名。
const PHRASES: &str = "custom_phrases";

/// 用户词文件名：造词框造的词（`qingjian-learning` 写在数据目录）与自造形码词
/// （`code_schemes::save_user_word` 写在 `code_dicts/` 下）同名，导出时两份都进 zip。
pub const USER_WORDS_FILE: &str = "user-words.tsv";

/// 合并两份用户词（`词\t拼音\t词频`，一行一条，`#` 开头是注释）：按词去重，同名同音以 `imported`
/// 为准，`local` 独有的词照原顺序接在后面。
///
/// 注释只留 `imported` 的——两边都是同一句表头，抄两遍没有意义。导入是「换电脑」和「补备份」
/// 两个场景共用的，谁都不该丢词，所以不整份替换。
pub fn merge_user_words(local: &str, imported: &str) -> String {
    let mut out = String::new();
    let mut seen: HashSet<String> = HashSet::new();
    for (text, first) in [(imported, true), (local, false)] {
        for line in text.lines() {
            let line = line.trim_end();
            let trimmed = line.trim_start();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.starts_with('#') {
                if first {
                    out.push_str(line);
                    out.push('\n');
                }
                continue;
            }
            let word = trimmed.split('\t').next().unwrap_or_default();
            if seen.insert(word.to_owned()) {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    out
}

/// 把配置文件的内容变成导出文件：注释与写法照旧，去掉短语、密钥与状态条位置。
pub fn export_settings(source: &str) -> Result<String, String> {
    let mut document = parse(source)?;
    document.remove(PHRASES);
    for (section, key) in LOCAL {
        if let Some(table) = document.get_mut(section).and_then(Item::as_table_like_mut) {
            table.remove(key);
        }
    }
    Ok(format!(
        "# 青简设置，在设置程序「高级」页导入；不含自定义短语、云服务密钥与状态条位置\n\n{document}"
    ))
}

/// 用导入的设置换掉现有配置：`current` 里的短语、密钥与状态条位置留着，其余照 `imported`。
/// 导入的内容读不成配置时报错，返回的新内容保证能读。
pub fn import_settings(current: &str, imported: &str) -> Result<String, String> {
    let mut document = parse(imported)?;
    toml::from_str::<Config>(imported)
        .map_err(|e| tr_args("settings_file.invalid", &[("error", &e.message())]))?;
    let current = parse(current)?;
    document.remove(PHRASES);
    if let Some(phrases) = current.get(PHRASES) {
        document.insert(PHRASES, phrases.clone());
    }
    for (section, key) in LOCAL {
        if let Some(table) = document.get_mut(section).and_then(Item::as_table_like_mut) {
            table.remove(key);
        }
        let kept = current
            .get(section)
            .and_then(Item::as_table_like)
            .and_then(|table| table.get(key))
            .cloned();
        if let Some(value) = kept {
            if document.get(section).is_none() {
                document.insert(section, Item::Table(toml_edit::Table::new()));
            }
            if let Some(table) = document.get_mut(section).and_then(Item::as_table_like_mut) {
                table.insert(key, value);
            }
        }
    }
    let merged = document.to_string();
    toml::from_str::<Config>(&merged)
        .map_err(|e| tr_args("settings_file.merge_failed", &[("error", &e.message())]))?;
    Ok(merged)
}

fn parse(source: &str) -> Result<DocumentMut, String> {
    source
        .parse::<DocumentMut>()
        .map_err(|e| tr_args("settings_file.unreadable", &[("error", &e.message())]))
}

#[cfg(test)]
mod tests {
    use super::{export_settings, import_settings, merge_user_words};

    const HERE: &str = "# 我的注释\n[general]\npage_size = 5\n\n[predict]\napi_key = \"sk-here\"\n\n[status_bar]\nx = 10\ny = 20\n\n[[custom_phrases]]\ncode = \"dz\"\ntext = \"地址\"\n";

    #[test]
    fn export_drops_phrases_secrets_and_screen_positions() {
        let exported = export_settings(HERE).unwrap();
        assert!(exported.contains("# 我的注释"));
        assert!(exported.contains("page_size = 5"));
        for gone in ["sk-here", "x = 10", "custom_phrases", "地址"] {
            assert!(!exported.contains(gone), "{gone}");
        }
    }

    #[test]
    fn import_keeps_local_phrases_secrets_and_positions() {
        let other = "[general]\npage_size = 7\n\n[predict]\napi_key = \"sk-other\"\n\n[[custom_phrases]]\ncode = \"ee\"\ntext = \"：\"\n";
        let merged = import_settings(HERE, other).unwrap();
        let config: crate::Config = toml::from_str(&merged).unwrap();
        assert_eq!(config.general.page_size(), 7);
        assert_eq!(config.predict.api_key.as_deref(), Some("sk-here"));
        assert_eq!(
            (config.status_bar.x, config.status_bar.y),
            (Some(10), Some(20))
        );
        assert_eq!(config.custom_phrases.len(), 1);
        assert_eq!(config.custom_phrases[0].code, "dz");
        assert!(import_settings(HERE, "[general]\npage_size = \"多\"\n").is_err());
        assert!(import_settings(HERE, "不是 toml").is_err());
    }

    /// 两边都有的词不重复，本机独有的词留着；表头只抄导入那份一次。
    #[test]
    fn user_words_merge_keeps_both_sides() {
        let header = "# 青简用户词：词\t拼音\t词频，与主词库同格式\n";
        let local = format!("{header}账套\tzhang tao\t100\n公司\tgong si\t100\n");
        let imported = format!("{header}公司\tgong si\t100\n甲方\tjia fang\t100\n");
        let merged = merge_user_words(&local, &imported);
        assert_eq!(merged.matches("# 青简用户词").count(), 1);
        assert_eq!(
            merged.lines().filter(|line| !line.starts_with('#')).count(),
            3
        );
        assert!(merged.contains("甲方\tjia fang\t100"));
        assert!(merged.contains("账套\tzhang tao\t100"));
        assert_eq!(merged.matches("公司").count(), 1);
    }

    /// 同一个词两边读音不一样时，用包里的那条（导入方是另一台电脑的最新状态）。
    #[test]
    fn user_words_merge_prefers_imported_reading() {
        let merged = merge_user_words("重\tchong\t100\n", "重\tzhong\t100\n");
        assert_eq!(merged, "重\tzhong\t100\n");
    }

    /// 一边没有词（新机器、或者从没造过词）时另一边的词原样出来。
    #[test]
    fn user_words_merge_with_empty_side() {
        assert_eq!(merge_user_words("", "重\tzhong\t100\n"), "重\tzhong\t100\n");
        assert_eq!(merge_user_words("重\tzhong\t100\n", ""), "重\tzhong\t100\n");
        assert_eq!(merge_user_words("", ""), "");
    }
}

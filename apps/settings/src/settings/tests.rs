//! 设置程序启动：配置文件写坏了也要打得开，下线的取值改写成现在的。

use bamboo_slip::App;
use qingjian_platform::{Config, LocalModel};

use super::Settings;
use crate::message::Message;

fn temp_config(name: &str, text: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("qingjian-settings-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(&path, text).unwrap();
    path
}

/// 语法错、类型错都按缺省值打开并记下错误，文件原样不动；每页都建得出来（顶上多一条提示）。
#[test]
fn a_broken_config_still_opens_with_defaults() {
    for (name, text) in [
        ("syntax", "[general\npage_size = 5\n"),
        ("type", "[general]\npage_size = \"五\"\n"),
    ] {
        let path = temp_config(name, text);
        let settings = Settings::load(path.clone());
        assert!(settings.config_error.is_some(), "{name}");
        assert_eq!(settings.config, Config::default(), "{name}");
        let _ = crate::view::index(&settings);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text, "{name}");
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}

#[test]
fn retired_zhiwei_is_rewritten_on_start() {
    let path = temp_config("zhiwei", "[model]\nenabled = true\nname = \"zhiwei\"\n");
    let settings = Settings::load(path.clone());
    assert!(settings.config_error.is_none());
    assert_eq!(settings.config.model.name, LocalModel::Tongbian);
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains("name = \"tongbian\"")
    );
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

/// 「句首不计入」：原来没有 `[ranking]` 段也能建出来，写进去的是布尔开关，来回切都落盘。
#[test]
fn the_ignore_sentence_start_switch_is_written_to_the_config() {
    let path = temp_config("ignore-start", "");
    let mut settings = Settings::load(path.clone());
    assert!(!settings.config.ranking.ignore_sentence_start);
    settings.update(Message::set("ranking", "ignore_sentence_start", true));
    assert!(settings.config.ranking.ignore_sentence_start);
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains("ignore_sentence_start = true")
    );
    settings.update(Message::set("ranking", "ignore_sentence_start", false));
    assert!(!settings.config.ranking.ignore_sentence_start);
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains("ignore_sentence_start = false")
    );
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

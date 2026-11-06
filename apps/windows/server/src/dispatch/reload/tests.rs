//! 词库文件变化在配置不变时仍应进入正在运行的引擎。

use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

use qingjian_core::Engine;
use qingjian_dictionary::{Dictionary, import};
use qingjian_learning::FrequencyLearner;
use qingjian_platform::{Config, NumpadDigit};

use super::CONFIG_POLL_INTERVAL;
use crate::dispatch::{DataDirs, Router, RouterConfig};

fn poll(router: &mut Router) {
    router.reload.as_mut().unwrap().last_check = Instant::now() - CONFIG_POLL_INTERVAL;
    router.poll_config_reload();
}

fn modified_at(path: &Path, seconds: u64) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(seconds))
        .unwrap();
}

#[test]
fn numpad_setting_reloads_and_invalid_value_preserves_previous_config() {
    let dir = std::env::temp_dir().join(format!("qingjian-numpad-reload-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(&path, "").unwrap();
    modified_at(&path, 100);
    let config = Config::load(&path).unwrap();
    let mut router = Router::new(
        Engine::new(Dictionary::default()),
        RouterConfig::from(&config),
    );
    router.watch_config(&config, path.clone(), dir.clone(), DataDirs::default());
    Config::set_value(&path, "general", "numpad_digit", "direct").unwrap();
    modified_at(&path, 200);
    poll(&mut router);
    assert_eq!(router.config.numpad_digit, NumpadDigit::Direct);
    Config::set_value(&path, "general", "numpad_digit", "invalid").unwrap();
    modified_at(&path, 300);
    poll(&mut router);
    assert_eq!(router.config.numpad_digit, NumpadDigit::Direct);
    Config::set_value(&path, "general", "numpad_digit", "select").unwrap();
    modified_at(&path, 400);
    poll(&mut router);
    assert_eq!(router.config.numpad_digit, NumpadDigit::Select);
    drop(router);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn import_replace_and_remove_without_config_changes() {
    let dir = std::env::temp_dir().join(format!("qingjian-reload-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let config_path = dir.join("config.toml");
    std::fs::write(&config_path, "").unwrap();
    let config = Config::load(&config_path).unwrap();
    let original_config = std::fs::read(&config_path).unwrap();
    let mut router = Router::new(Engine::new(Dictionary::default()), RouterConfig::default());
    router.watch_config(
        &config,
        config_path.clone(),
        dir.clone(),
        DataDirs {
            user_root: Some(dir.clone()),
            user_dicts: Some(dir.join("dicts")),
            ..DataDirs::default()
        },
    );

    let source = dir.join("law.dict.yaml");
    std::fs::write(&source, "---\nname: law\n...\n合同法\the tong fa\t120\n").unwrap();
    let imported = import::import(&source, &dir.join("dicts")).unwrap();
    modified_at(&imported.path, 100);
    poll(&mut router);
    let dictionaries = router.engine.extra_dictionaries();
    assert_eq!(dictionaries.len(), 1);
    assert_eq!(
        dictionaries[0].lookup(&["he", "tong", "fa"], false)[0].text,
        "合同法"
    );

    // 只改词频，输出长度保持不变，必须靠 mtime 发现更新。
    let original_len = std::fs::metadata(&imported.path).unwrap().len();
    std::fs::write(&source, "---\nname: law\n...\n合同法\the tong fa\t121\n").unwrap();
    import::import(&source, &dir.join("dicts")).unwrap();
    modified_at(&imported.path, 200);
    assert_eq!(
        std::fs::metadata(&imported.path).unwrap().len(),
        original_len
    );
    poll(&mut router);
    assert_eq!(
        router.engine.extra_dictionaries()[0].lookup(&["he", "tong", "fa"], false)[0].frequency,
        121
    );

    // 保持旧词库映射打开，模拟运行中的 Server 收到同名替换。
    std::fs::write(
        &source,
        "---\nname: law\n...\n民法典\tmin fa dian\t100\n法律\tfa lv\t30\n",
    )
    .unwrap();
    import::import(&source, &dir.join("dicts")).unwrap();
    poll(&mut router);
    let dictionary = &router.engine.extra_dictionaries()[0];
    assert!(dictionary.lookup(&["he", "tong", "fa"], false).is_empty());
    assert_eq!(
        dictionary.lookup(&["min", "fa", "dian"], false)[0].text,
        "民法典"
    );

    // YAML 头中只有引用、没有本文件词条时，失败不能覆盖已有词库。
    std::fs::write(&source, "---\nname: law\nimport_tables:\n  - other\n...\n").unwrap();
    assert!(import::import(&source, &dir.join("dicts")).is_err());
    assert_eq!(Dictionary::from_path(&imported.path).unwrap().len(), 2);

    let removed = dir.join("dicts/removed");
    std::fs::create_dir_all(&removed).unwrap();
    std::fs::rename(&imported.path, removed.join("law.qj")).unwrap();
    poll(&mut router);
    assert!(router.engine.extra_dictionaries().is_empty());
    assert_eq!(std::fs::read(&config_path).unwrap(), original_config);
    drop(router);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn dictionary_changes_do_not_retry_broken_config() {
    let dir = std::env::temp_dir().join(format!("qingjian-reload-broken-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let config_path = dir.join("config.toml");
    std::fs::write(&config_path, "[general]\npage_size = 5\n").unwrap();
    modified_at(&config_path, 100);
    let config = Config::load(&config_path).unwrap();
    let mut router = Router::new(
        Engine::new(Dictionary::default()),
        RouterConfig::from(&config),
    );
    router.watch_config(
        &config,
        config_path.clone(),
        dir.clone(),
        DataDirs {
            user_root: Some(dir.clone()),
            user_dicts: Some(dir.join("dicts")),
            ..DataDirs::default()
        },
    );

    std::fs::write(&config_path, "[broken").unwrap();
    modified_at(&config_path, 200);
    poll(&mut router);
    let source = dir.join("law.dict.yaml");
    std::fs::write(&source, "---\nname: law\n...\n合同法\the tong fa\t120\n").unwrap();
    let imported = import::import(&source, &dir.join("dicts")).unwrap();
    poll(&mut router);
    assert_eq!(router.config.page_size, 5);
    assert_eq!(router.engine.extra_dictionaries().len(), 1);

    // 保持坏配置的 mtime，以可观察的配置值确认后续轮询不会重新读它。
    std::fs::write(&config_path, "[general]\npage_size = 9\n").unwrap();
    modified_at(&config_path, 200);
    poll(&mut router);
    assert_eq!(router.config.page_size, 5);
    let removed = dir.join("dicts/removed");
    std::fs::create_dir_all(&removed).unwrap();
    std::fs::rename(&imported.path, removed.join("law.qj")).unwrap();
    poll(&mut router);
    assert_eq!(router.config.page_size, 5);
    assert!(router.engine.extra_dictionaries().is_empty());

    modified_at(&config_path, 300);
    poll(&mut router);
    assert_eq!(router.config.page_size, 9);
    drop(router);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// 设置程序导入了造词备份：用户词文件一变，引擎整份换一份学习数据（不用重启输入法），
/// 内存里还没落盘的旧词跟着消失——磁盘上那份已经是合并后的结果。
#[test]
fn user_words_are_reread_when_the_file_changes() {
    let dir =
        std::env::temp_dir().join(format!("qingjian-user-words-reload-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let config_path = dir.join("config.toml");
    std::fs::write(&config_path, "").unwrap();
    modified_at(&config_path, 100);
    let words = dir.join("user-words.tsv");
    std::fs::write(&words, "# 青简用户词\n账套\tzhang tao\t100\n").unwrap();

    let config = Config::load(&config_path).unwrap();
    let learner = FrequencyLearner::from_path(dir.join("user.tsv")).unwrap();
    let mut engine = Engine::new(Dictionary::default()).with_learner(Box::new(learner));
    // 只在内存里造的词：重读之后应该不在
    engine
        .learn_word_with_reading("内存词", &["nei".into(), "cun".into(), "ci".into()])
        .unwrap();
    let mut router = Router::new(engine, RouterConfig::from(&config));
    router.watch_config(
        &config,
        config_path.clone(),
        dir.clone(),
        DataDirs {
            user_root: Some(dir.clone()),
            ..DataDirs::default()
        },
    );

    // 设置程序写入合并后的用户词
    std::fs::write(
        &words,
        "# 青简用户词\n账套\tzhang tao\t100\n青简\tqing jian\t100\n",
    )
    .unwrap();
    modified_at(&words, 400);
    poll(&mut router);

    let user_words = router.engine.learner_mut().user_words().unwrap();
    assert_eq!(user_words.lookup(&["qing"], true)[0].text, "青简");
    assert_eq!(user_words.lookup(&["zhang"], true)[0].text, "账套");
    assert!(user_words.lookup(&["nei"], true).is_empty());

    drop(router);
    let _ = std::fs::remove_dir_all(&dir);
}

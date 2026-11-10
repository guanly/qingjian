//! 菜单动作：开关类只改 config.toml（或触发一次性操作），改完由 apply_config 统一生效。

use super::diagnostics::open_with_system;
use super::*;
use qingjian_core::LearnedWord;
use qingjian_platform::code_schemes;

impl Host {
    /// 菜单动作。开关类先落盘再热加载，菜单勾选状态永远来自文件里的值。
    pub fn perform(&mut self, action: MenuAction) {
        tracing::info!(?action, "菜单");
        match action {
            MenuAction::ToggleCloud => {
                let on = !self.settings.config().predict.enabled;
                if self.settings.set_bool("predict", "enabled", on) {
                    self.apply_config(false);
                }
            }
            MenuAction::ToggleFuzzy(index) => {
                let name = FuzzyRules::NAMES[index];
                let on = !self.settings.config().fuzzy.is_on(name);
                if self.settings.set_bool("fuzzy", name, on) {
                    self.apply_config(false);
                }
            }
            MenuAction::OpenSettings => match settings_app() {
                Some(app) => open_with_system(&[&app.to_string_lossy()]),
                None => tracing::error!("包里没有设置程序，设置打不开"),
            },
            // 配置目录：`config.toml`、用户词、导入的词库与码表、主题都在这儿。目录不在时顺手建出来，
            // 免得用户点开发现是空的、或者访达直接报找不到
            MenuAction::OpenConfigDir => {
                if let Some(dir) = crate::app::paths::user_data_dir() {
                    open_with_system(&[&dir.to_string_lossy()]);
                }
            }
            MenuAction::OpenDownload => open_with_system(&[qingjian_update::DOWNLOAD_URL]),
            MenuAction::NewWord => self.new_word(),
            // 要当前会话的文本客户端，控制器在 menuAction: 里先接走了
            MenuAction::OpenSymbolPanel => {}
        }
    }

    /// 造词：弹框收一批中文词（一行一个），读音与形码编码由引擎算好，存完立刻落盘——用户马上要用。
    /// 菜单「造词…」与 `[shortcut] new_word` 都走这里。
    ///
    /// 框是非模态的（`crate::dialog`），但开框会把本进程激活成前台应用，IMK 随即给这个窗口建输入会话，
    /// 那次 `activateServer` 不能撞在 `Host` 的借用上，所以开框这一步排到主队列，存的时候再借回来。
    pub fn new_word(&mut self) {
        let prefill = self
            .engine
            .recent_commits(1)
            .first()
            .map(|(text, _)| text.clone())
            .unwrap_or_default();
        defer(move || {
            new_word::ask(&prefill, |words| {
                if !words.is_empty() {
                    with(|host| host.learn_new_word(&words));
                }
            });
        });
    }

    /// 造词真正落地的部分：逐个算读音与编码、写用户词与形码用户词，最后回一句提示。
    ///
    /// 一批里有的词失败不影响其余——失败的攒起来在最后一次性说清楚是哪个词、为什么。
    fn learn_new_word(&mut self, words: &[String]) {
        let mut learned = Vec::with_capacity(words.len());
        let mut failed = Vec::new();
        for word in words {
            match self.engine.learn_user_word(word) {
                Ok(word) => {
                    // 形码方案下再写一条形码用户词：拼音侧与五笔侧都能打出这个词
                    self.save_code_word(&word);
                    tracing::info!(
                        text = %word.text,
                        reading = ?word.reading,
                        code = ?word.code,
                        "造词写入用户词"
                    );
                    learned.push(word);
                }
                Err(reason) => {
                    tracing::warn!(%reason, %word, "造词失败");
                    failed.push(format!("{word}：{reason}"));
                }
            }
        }
        if !learned.is_empty() {
            // 一批一起落盘：一个词一次刷盘没必要
            self.engine.flush_learning();
            // 造词框关掉后不弹别的框，借模式提示回一句：用户得知道存上了没有
            match learned.as_slice() {
                [one] => self.osd.show(&format!("已造词 {}", one.text)),
                many => self.osd.show(&format!("已造词 {} 个词", many.len())),
            }
        }
        if !failed.is_empty() {
            // 报错框同样是非模态的，借出去就得等它关掉，一样排到主队列
            let reason = failed.join("\n");
            defer(move || new_word::report(&reason));
        }
    }

    /// 把造出来的编码写进用户形码词库（`code_dicts/user-words.tsv`）；壳每秒比对目录，
    /// 文件一变方案就重载，不用重启。拼音输入、或方案没有词码规则时没有编码，跳过。
    fn save_code_word(&self, learned: &LearnedWord) {
        let Some(code) = &learned.code else {
            return;
        };
        let Some(scheme) = self.settings.config().general.code() else {
            return;
        };
        let Some(dir) = paths::user_data_dir() else {
            return;
        };
        match code_schemes::save_user_word(&dir, &scheme, &learned.text, code) {
            Ok(path) => tracing::info!(path = %path.display(), code, "造词写入形码词库"),
            Err(error) => tracing::warn!(%error, "形码用户词写不进，五笔侧暂时打不出这个词"),
        }
    }

    /// 标点切换键：翻转中文模式的全半角标点，落盘后热加载。英文模式一律半角，调用方不在英文模式下调。
    pub fn toggle_punctuation(&mut self) {
        let full_width = !self.settings.config().general.full_width_punctuation;
        tracing::debug!(full_width, "切换全角标点");
        if self
            .settings
            .set_bool("general", "full_width_punctuation", full_width)
        {
            self.apply_config(false);
        }
    }
}

/// 随包的设置程序；直接跑二进制（不在 .app 里）或包里缺它时为 `None`。
fn settings_app() -> Option<std::path::PathBuf> {
    let bundle = objc2_foundation::NSBundle::mainBundle()
        .bundlePath()
        .to_string();
    let app = std::path::Path::new(&bundle).join("Contents/Applications/QingjianSettings.app");
    app.is_dir().then_some(app)
}

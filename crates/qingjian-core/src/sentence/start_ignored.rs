//! 「句首不计入」开关：打开后句首不看语料的 `<s>` 二元，一律按一元概率算。
//!
//! 语料里有些词大量出现在句首（条目名、页面标题、页脚），`c(<s>, 词) / c(<s>)` 把它们抬得比同音词高，
//! 而用户自己的行文里未必这么用。打开这个开关，句首就只让词频说话，句中照常走 bigram / trigram。
//!
//! 开关由 [`crate::Engine`] 持有，与这里的包装层共享：配置改了下一次查词就生效，不用重建模型。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::LanguageModel;

/// 句首不看 `<s>` 的开关，包装在一个语言模型外面。
pub struct StartIgnored {
    /// 被包装的模型。
    inner: Box<dyn LanguageModel>,

    /// 与 Engine 共享的开关，见 [`Self::new`]。
    enabled: Arc<AtomicBool>,
}

impl StartIgnored {
    /// `enabled` 要与 Engine 手上那份是同一个（`Arc::clone`），配置热更新才传得进来。
    pub fn new(inner: Box<dyn LanguageModel>, enabled: Arc<AtomicBool>) -> Self {
        Self { inner, enabled }
    }

    /// 现在是不是句首不计入。
    fn ignores(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// 一元概率。实现给不出一元时退回普通句首概率：开关在这个模型上不起作用，但不会比原来更差。
    fn unigram(&self, word: &str) -> Option<f64> {
        self.inner
            .unigram_log_prob(word)
            .or_else(|| self.inner.log_prob(None, word))
    }
}

impl LanguageModel for StartIgnored {
    fn log_prob(&self, previous: Option<&str>, word: &str) -> Option<f64> {
        if previous.is_none() && self.ignores() {
            return self.unigram(word);
        }
        self.inner.log_prob(previous, word)
    }

    fn log_prob_trigram(
        &self,
        earlier: Option<&str>,
        previous: Option<&str>,
        word: &str,
    ) -> Option<f64> {
        if previous.is_none() && self.ignores() {
            return self.unigram(word);
        }
        self.inner.log_prob_trigram(earlier, previous, word)
    }

    fn unigram_log_prob(&self, word: &str) -> Option<f64> {
        self.inner.unigram_log_prob(word)
    }

    fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 句首给 0.5、一元给 0.01 的假模型：两者差得远，看得出用的是哪一条。
    struct Bumpy;

    impl LanguageModel for Bumpy {
        fn log_prob(&self, previous: Option<&str>, word: &str) -> Option<f64> {
            (word == "岩崎").then(|| if previous.is_none() { 0.5_f64 } else { 0.02 }.ln())
        }

        fn unigram_log_prob(&self, word: &str) -> Option<f64> {
            (word == "岩崎").then(|| 0.01_f64.ln())
        }
    }

    fn wrapped(on: bool) -> (StartIgnored, Arc<AtomicBool>) {
        let enabled = Arc::new(AtomicBool::new(on));
        (
            StartIgnored::new(Box::new(Bumpy), Arc::clone(&enabled)),
            enabled,
        )
    }

    #[test]
    fn the_start_falls_back_to_unigram_when_on() {
        let (model, _) = wrapped(true);
        let start = model.log_prob(None, "岩崎").unwrap();
        assert!((start - 0.01_f64.ln()).abs() < 1e-12, "{start}");
        // 句中照旧走 bigram
        let middle = model.log_prob(Some("车站"), "岩崎").unwrap();
        assert!((middle - 0.02_f64.ln()).abs() < 1e-12, "{middle}");
    }

    #[test]
    fn the_start_keeps_the_bigram_when_off() {
        let (model, _) = wrapped(false);
        let start = model.log_prob(None, "岩崎").unwrap();
        assert!((start - 0.5_f64.ln()).abs() < 1e-12, "{start}");
    }

    #[test]
    fn trigram_entry_ignores_start_too() {
        let (model, _) = wrapped(true);
        let start = model.log_prob_trigram(None, None, "岩崎").unwrap();
        assert!((start - 0.01_f64.ln()).abs() < 1e-12, "{start}");
    }

    /// 开关与 Engine 共享：换了它不用重建包装层。
    #[test]
    fn the_switch_can_change_after_wrapping() {
        let (model, enabled) = wrapped(false);
        assert!((model.log_prob(None, "岩崎").unwrap() - 0.5_f64.ln()).abs() < 1e-12);
        enabled.store(true, Ordering::Relaxed);
        assert!((model.log_prob(None, "岩崎").unwrap() - 0.01_f64.ln()).abs() < 1e-12);
    }

    /// 实现不给一元时退回句首概率，开关不生效但不会算出更差的结果。
    #[test]
    fn model_without_unigram_keeps_the_start_probability() {
        struct NoUnigram;
        impl LanguageModel for NoUnigram {
            fn log_prob(&self, _previous: Option<&str>, word: &str) -> Option<f64> {
                (word == "岩崎").then(|| 0.5_f64.ln())
            }
        }
        let model = StartIgnored::new(Box::new(NoUnigram), Arc::new(AtomicBool::new(true)));
        assert!((model.log_prob(None, "岩崎").unwrap() - 0.5_f64.ln()).abs() < 1e-12);
    }
}

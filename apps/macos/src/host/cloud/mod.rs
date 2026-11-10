//! 云端：联想的等待 / 取消 / 轮询与结果套用到当前页。

mod predict_monitor;

pub(super) use predict_monitor::PredictMonitor;

use super::*;

impl Host {
    /// 已发出联想请求：清掉旧结果，开始轮询。`manual` 是按需联想的请求序号，自动联想传 `None`。
    pub fn await_prediction(&mut self, manual: Option<u64>) {
        self.sentence = None;
        self.manual_prediction = manual;
        self.monitor.start();
    }

    /// 作废联想：不再轮询，丢掉已到的整句。
    pub fn cancel_prediction(&mut self) {
        self.engine.cancel_prediction();
        self.monitor.stop();
        self.sentence = None;
        self.manual_prediction = None;
    }

    /// 定时器回调：结果到了就画上去。云端词补进第一页末尾，整句挂在 preedit 右侧。
    pub fn poll_prediction(&mut self) {
        let Some(prediction) = self.engine.poll_prediction() else {
            if self.monitor.expired() {
                if let Some(job) = self.candidate_translation.take() {
                    self.engine.cancel_candidate_translation(job.sequence);
                    self.status = Some("模型生成译文超时，请重试".to_owned());
                    self.render();
                }
                self.monitor.stop();
            }
            return;
        };
        self.apply_prediction(prediction);
        if self.candidate_translation.is_none() {
            self.monitor.stop();
        }
    }

    pub(super) fn apply_prediction(&mut self, prediction: Prediction) {
        if self
            .candidate_translation
            .as_ref()
            .is_some_and(|job| job.sequence == prediction.sequence)
        {
            let job = self.candidate_translation.take().expect("刚检查过候选译文");
            self.monitor.stop();
            self.status = match prediction.sentence {
                Some(generated)
                    if self
                        .engine
                        .set_generated_candidate_translation(&job.candidate, &generated) =>
                {
                    Some(format!("已更新「{}」的译文", job.candidate.text))
                }
                _ => Some("模型没有生成可用译文".to_owned()),
            };
            for candidate in self.session.layout.local_mut() {
                if candidate.text == job.candidate.text {
                    candidate.translation = self.engine.candidate_translation(&candidate.text);
                }
            }
            for candidate in self.session.layout.cloud_mut() {
                if candidate.text == job.candidate.text {
                    candidate.translation = self.engine.candidate_translation(&candidate.text);
                }
            }
            self.render();
            return;
        }
        // 翻译选中文字：译文作为唯一候选摆进窗口，等用户回车替换或 Esc 放弃
        if let Some(job) = self.translation.as_mut() {
            match prediction.sentence {
                Some(text) => {
                    job.result = Some(text.clone());
                    self.reset_session(None, vec![cloud_candidate(text)]);
                    self.render();
                }
                None => {
                    tracing::info!("云端没有给出译文");
                    self.end_translation();
                }
            }
            return;
        }
        // 符号面板开着时格子里是面板条目，云端词不往里补
        if self.engine.composition().is_empty() || self.engine.symbol_panel().is_some() {
            return;
        }
        // 按需联想的结果排在第一页最前面，高亮回到第一个，与 Windows 一致
        let manual = self.manual_prediction.take() == Some(prediction.sequence);
        // 自动联想的云端词补进第一页末尾（前面的本地候选不动），整句挂在 preedit 右侧；云端词没有译文，先补上。
        // 用户已经翻到后面的页、或高亮已经移到会被挪走的那几格时不补：那几格正被他看着 / 要选
        let layout = &self.session.layout;
        let untouched = cloud_slots_untouched(
            self.session.page,
            self.session.highlighted,
            layout.local().is_empty(),
            layout.page_size(),
            layout.capacity(),
        );
        if (manual || untouched) && !prediction.words.is_empty() {
            let mut words = qingjian_core::CandidateList {
                items: prediction
                    .words
                    .into_iter()
                    .map(CloudWord::into_candidate)
                    .collect(),
            };
            self.engine.annotate(&mut words);
            let filled = if manual {
                self.session.page = 0;
                self.session.highlighted = 0;
                self.session.navigated = false;
                self.session.layout.set_cloud_first(words.items)
            } else {
                self.session.layout.set_cloud(words.items)
            };
            tracing::debug!(filled, manual, "云端词已补进候选");
        }
        self.sentence = prediction.sentence;
        self.render();
    }
}

/// 用系统的 `open` 打开文件或目录；输入法进程没有自己的文档窗口，交给访达 / 默认编辑器最省事。
/// 云端词到了还能不能补进第一页：用户还在第一页，且高亮没落在会被云端词顶掉的那几格上。
/// 没有本地候选（问字模式）时整页都是云端的，谈不上挪走谁，永远能补——
/// 之前没分这种情况，`page_size - capacity` 算出 0，问字的答案全被当成「会打扰用户」丢掉了。
pub(super) fn cloud_slots_untouched(
    page: usize,
    highlighted: usize,
    local_empty: bool,
    page_size: usize,
    capacity: usize,
) -> bool {
    page == 0 && (local_empty || highlighted < page_size.saturating_sub(capacity))
}

/// 翻译窗口里的一行：译文（或占位文字）当作云端来源的候选画出来。
pub(super) fn cloud_candidate(text: String) -> Candidate {
    Candidate {
        text,
        kind: CandidateKind::Cloud,
        syllables: Vec::new(),
        reading: None,
        translation: None,
        aux_code: None,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn question_answers_fill_the_page_even_though_every_slot_is_cloud() {
        // 问字：没有本地候选，capacity == page_size
        assert!(super::cloud_slots_untouched(0, 0, true, 9, 9));
        // 组句：高亮在前面的本地格上能补，高亮已在末尾两格（云端要占的位置）不补，翻页后不补
        assert!(super::cloud_slots_untouched(0, 0, false, 9, 2));
        assert!(!super::cloud_slots_untouched(0, 7, false, 9, 2));
        assert!(!super::cloud_slots_untouched(1, 0, false, 9, 2));
    }
}

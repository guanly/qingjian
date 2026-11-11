//! 单击 Shift 切换中 / 英（配置 `[shortcut] switch_mode` 勾了 shift 时），以及按应用固定中英
//! （配置 `[apps] fixed_chinese` / `fixed_english`）。
//!
//! 边沿从修饰键事件来：控制器在 `recognizedEvents:` 里声明了 flagsChanged
//! （缺省只回 `NSKeyDownMask`，不声明就一个修饰键事件都收不到，Shift 的按下 / 抬起只能靠轮询采样，
//! 而采样在「按下与抬起落在同一拍之间」时会把整次点击漏掉），拿到精确边沿后在这里判定
//! 「按下到抬起之间没有别的键插进来」= 一次单击。规则与 Windows 的 `com/key/tap.rs` 相同，但那边能看到
//! 被系统热键吃掉的键并作废这次单击，macOS 看不到（系统热键不进 `handleEvent:`），
//! 所以「按住 Shift 按了系统快捷键再松开」会被误判成一次单击。
//! 勾上之后中英由本模块决定，Caps Lock 退化成只管大小写。

use std::time::{Duration, Instant};

use crate::imk::modifiers;

use crate::host::Host;

/// 切换之后这么久内「完成」的一整次单击才算抖动、丢掉：有些外接键盘松开 Shift 后 8–10 ms 又补一对
/// 按下 / 抬起，不挡的话一次单击会切过去又切回来。判据是整对边沿都挤在这个窗口里——人手不可能在 50 ms
/// 内完成松开→按下→松开，而补的那一对 20 ms 内就结束了。只按「按下时刻在窗口内」挡会把快速连按的
/// 第二下整次吃掉（按下被丢 → 抬起时没有待定项），所以窗口看的是抬起时刻。
const BOUNCE_WINDOW: Duration = Duration::from_millis(50);

/// 按住超过这么久再松开不算单击：鼠标事件不进 IMK 会话，按住 Shift 拖选文字时没有别的键能作废这次单击。
const TAP_WINDOW: Duration = Duration::from_millis(300);

/// 左右两个 Shift 的键码。只有 Shift 自己不算「插进来的别的键」：按下它本来就是这次单击的一部分；
/// 别的修饰键（Cmd / Ctrl / Option / Caps Lock）按下一律作废，与 Windows 的 `tap_key` 同口径。
fn is_shift_key(key_code: u16) -> bool {
    matches!(key_code, 56 | 60)
}

/// 单击 Shift 的判定与它切出来的持久英文模式。
#[derive(Default)]
pub struct ModeSwitch {
    /// 配置里勾了「单击 Shift」（`[shortcut] switch_mode` 含 shift）。
    enabled: bool,

    /// Shift 按下之后还没有别的键插进来时，按下的时刻；抬起时没超过 [`TAP_WINDOW`] 就算一次单击。
    pending: Option<Instant>,

    /// 上次切换的时刻，[`BOUNCE_WINDOW`] 内完成的单击当抖动丢掉。
    toggled_at: Option<Instant>,

    /// 单击 Shift 切出来的持久英文模式；`enabled` 为假时无意义（`set_enabled(false)` 会清零）。
    english: bool,

    /// 按应用固定的模式（`[apps] fixed_chinese` / `fixed_english`）：`Some(true)` 英文、`Some(false)` 中文、
    /// `None` 不管。切进应用时设定，用户在那个应用里按 Caps Lock / 单击 Shift 后清掉。
    forced: Option<bool>,

    /// 设 [`Self::forced`] 那一刻的 Caps Lock 状态：它变了就说明用户按过 Caps Lock。
    forced_caps: bool,
}

impl ModeSwitch {
    /// 配置里勾了「单击 Shift」。
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// 单击 Shift 切出来的英文模式。
    pub fn english(&self) -> bool {
        self.english
    }

    /// 配置里的开关变了。
    pub fn set_enabled(&mut self, enabled: bool) {
        if self.enabled == enabled {
            return;
        }
        self.enabled = enabled;
        // 关掉时退回中文，免得留着上次切出来的英文模式
        if !enabled {
            self.english = false;
        }
    }

    /// 切进应用时按 `[apps]` 的名单固定中英：`english` 是固定成哪种，`None` 就是不固定；`caps` 是此刻的
    /// Caps Lock 状态，之后它一变就当用户自己接管了。
    pub fn force(&mut self, english: Option<bool>, caps: bool) {
        self.forced = english;
        self.forced_caps = caps;
    }

    /// 当前有效的固定模式；Caps Lock 与设定时不同（用户按过）就不算数，交回给 Caps Lock。
    pub fn forced_mode(&self, caps: bool) -> Option<bool> {
        self.forced.filter(|_| caps == self.forced_caps)
    }

    /// Caps Lock 动过了：这个应用里不再固定，之后跟着 Caps Lock 走。返回是否真的放掉了。
    pub fn release_forced(&mut self, caps: bool) -> bool {
        if self.forced.is_some() && caps != self.forced_caps {
            self.forced = None;
            return true;
        }
        false
    }

    /// 焦点切换 / 停用：忘掉这次还没抬起的 Shift，
    /// 免得「按着 Shift 切走再切回、然后松开」被当成一次单击。
    pub fn reset(&mut self) {
        self.pending = None;
    }

    /// 任一普通按键按下：Shift 按下到抬起之间插了别的键，这次不算单击。
    pub fn note_key(&mut self) {
        self.pending = None;
    }

    /// 修饰键事件：`key_code` 那次按下（`down`）。构成一次「单击 Shift」返回 `true`。
    /// `current` 是此刻显示给用户的中英模式（按应用固定时可能与 `english` 字段不同），切换从它翻。
    pub fn shift_event(&mut self, key_code: u16, down: bool, current: bool) -> bool {
        self.shift_event_at(key_code, down, Instant::now(), current)
    }

    fn shift_event_at(&mut self, key_code: u16, down: bool, now: Instant, current: bool) -> bool {
        if !self.enabled {
            return false;
        }
        // 别的修饰键（Cmd / Ctrl / Option / Caps Lock）：这次不算单击
        if !is_shift_key(key_code) {
            self.pending = None;
            return false;
        }
        if down {
            self.pending = Some(now);
            return false;
        }
        let Some(at) = self.pending.take() else {
            return false;
        };
        // 按住太久再松开不算单击
        if now.duration_since(at) > TAP_WINDOW {
            return false;
        }
        // 换完的窗口里又完成了一整次单击 → 键盘补的那一对，丢掉；此时不记 `toggled_at`，
        // 免得抖动把这窗口一直续下去
        if self
            .toggled_at
            .is_some_and(|toggled| now.duration_since(toggled) < BOUNCE_WINDOW)
        {
            return false;
        }
        self.english = !current;
        // 单击 Shift 就是用户要自己定：按应用固定的模式从这一下起让位
        self.forced = None;
        self.toggled_at = Some(now);
        true
    }
}

impl Host {
    /// 中英模式：按应用固定时以它为准；勾了「单击 Shift」时由 Shift 的切换决定（Caps Lock 退化成只管大小写），
    /// 没勾看 Caps Lock。
    pub fn english_mode(&self) -> bool {
        let caps = modifiers::caps_lock_on();
        if let Some(forced) = self.mode_switch.forced_mode(caps) {
            return forced;
        }
        if self.mode_switch.enabled() {
            self.mode_switch.english()
        } else {
            caps
        }
    }

    /// 切到一个应用：`[apps] fixed_chinese` / `fixed_english` 里列了它就固定中英。模式真的变了才闪一次
    /// 「中」/「英」，免得每切一次应用都弹。
    pub fn apply_app_mode(&mut self) {
        let wanted = self.apps.fixed_mode(self.engine.application());
        let before = self.english_mode();
        self.mode_switch.force(wanted, modifiers::caps_lock_on());
        let after = self.english_mode();
        if before == after {
            return;
        }
        tracing::info!(
            english = after,
            app = self.engine.application().unwrap_or(""),
            "按应用固定中英"
        );
        self.osd.show(if after { "英" } else { "中" });
        // 已经提示过了：刷新状态时不要再闪一次
        self.last_osd_english = Some(after);
    }

    /// 英文模式下一个字母要不要大写：勾了「单击 Shift」时 Caps Lock 只管大小写（与 Shift 异或）；
    /// 没勾时 Caps Lock 就是中英开关，不参与大小写。
    pub fn english_letter_uppercase(&self) -> bool {
        if self.mode_switch.enabled() {
            modifiers::shift_down() != modifiers::caps_lock_on()
        } else {
            modifiers::shift_down()
        }
    }

    /// 收到了修饰键事件（`recognizedEvents:` 里的 flagsChanged）：Shift 的按下 / 抬起在这里判定，
    /// 这次切换了中英返回 `true`。
    pub fn shift_event(&mut self, key_code: u16, down: bool) -> bool {
        // 按应用固定时 `english` 字段可能不是屏幕上那个模式，从显示出来的模式翻
        let current = self.english_mode();
        if !self.mode_switch.shift_event(key_code, down, current) {
            return false;
        }
        tracing::info!(english = self.english_mode(), "单击 Shift 切换中英");
        self.refresh_mode_ui();
        self.render();
        true
    }

    /// 中英模式或 Caps Lock 可能变了：刷新候选窗口的输入状态，变了就弹一次右上角提示。
    ///
    /// 中英变了显示「中」/「英」（优先）；只有大小写变了才显示「大写」/「小写」——后者出现在勾了
    /// 「单击 Shift」之后，那时 Caps Lock 退化成只管大小写。
    pub fn refresh_mode_ui(&mut self) {
        let english = self.english_mode();
        let caps = modifiers::caps_lock_on();
        // 第一次只记不弹：启动或刚激活时不该凭空冒一个提示
        if self.last_osd_english.is_some_and(|last| last != english) {
            self.osd.show(if english { "英" } else { "中" });
        } else if self.last_osd_caps.is_some_and(|last| last != caps) {
            self.osd.show(if caps { "大写" } else { "小写" });
        }
        self.last_osd_english = Some(english);
        self.last_osd_caps = Some(caps);
    }

    /// 停用时忘掉已提示过的状态：下次激活的第一次刷新只记不弹，焦点来回挪不至于弹提示。
    pub fn forget_osd_state(&mut self) {
        self.last_osd_english = None;
        self.last_osd_caps = None;
    }

    /// 任一按键按下：除 Shift 自己以外的任何键（含别的修饰键）都作废待定的单击 Shift
    /// （Shift + 字母 / Shift + Cmd 组合都不该被当成单击）。
    ///
    /// 顺带看 Caps Lock 有没有被按过：固定在某个模式的应用里，用户按 Caps Lock 就是要自己接管，
    /// 从这一下起交回给 Caps Lock。
    pub fn note_key(&mut self, key_code: u16) {
        if !is_shift_key(key_code) {
            self.mode_switch.note_key();
        }
        self.mode_switch.release_forced(modifiers::caps_lock_on());
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::ModeSwitch;

    const LEFT_SHIFT: u16 = 56;

    fn enabled() -> ModeSwitch {
        let mut switch = ModeSwitch::default();
        switch.set_enabled(true);
        switch
    }

    /// 在这个时刻单击一次 Shift（按下、`hold` 毫秒后抬起），返回这次算不算一次单击。`current` 是此刻显示的模式。
    fn tap_held_at(switch: &mut ModeSwitch, at: Instant, hold: Duration, current: bool) -> bool {
        assert!(
            !switch.shift_event_at(LEFT_SHIFT, true, at, current),
            "按下不算一次单击"
        );
        switch.shift_event_at(LEFT_SHIFT, false, at + hold, current)
    }

    /// 按下、60 ms 后抬起的一次单击。
    fn tap_at(switch: &mut ModeSwitch, at: Instant, current: bool) -> bool {
        tap_held_at(switch, at, Duration::from_millis(60), current)
    }

    #[test]
    fn a_bounce_right_after_switching_is_ignored() {
        let mut switch = enabled();
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        let current = switch.english();
        assert!(tap_at(&mut switch, at(0), current));
        assert!(switch.english());
        // 松开（60 ms）后 10 ms 补来的一对按下 / 抬起
        let current = switch.english();
        assert!(!switch.shift_event_at(LEFT_SHIFT, true, at(70), current));
        assert!(!switch.shift_event_at(LEFT_SHIFT, false, at(75), current));
        assert!(switch.english(), "抖动不该把模式切回去");
        // 冷却过后的下一次单击照常切回
        let current = switch.english();
        assert!(tap_at(&mut switch, at(400), current));
        assert!(!switch.english());
    }

    #[test]
    fn a_fast_double_tap_toggles_twice() {
        let mut switch = enabled();
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        // 两下单击间隔 40 ms、每下按住 50 ms：整轮 140 ms，是快手的真实速度
        let current = switch.english();
        assert!(tap_held_at(
            &mut switch,
            at(0),
            Duration::from_millis(50),
            current
        ));
        assert!(switch.english());
        let current = switch.english();
        assert!(tap_held_at(
            &mut switch,
            at(90),
            Duration::from_millis(50),
            current
        ));
        assert!(!switch.english(), "第二下也要生效");
    }

    #[test]
    fn a_tap_starting_in_the_bounce_window_but_ending_outside_counts() {
        let mut switch = enabled();
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        let current = switch.english();
        assert!(tap_at(&mut switch, at(0), current));
        // 抬起在 60 ms；第二下 70 ms 就按下（落在抖动窗口里），140 ms 才抬起 → 是真人，不是抖动
        let current = switch.english();
        assert!(!switch.shift_event_at(LEFT_SHIFT, true, at(70), current));
        assert!(
            switch.shift_event_at(LEFT_SHIFT, false, at(140), current),
            "按下落在窗口内、抬起在窗口外，算真的单击"
        );
        assert!(!switch.english());
    }

    #[test]
    fn holding_shift_too_long_is_not_a_tap() {
        let mut switch = enabled();
        let start = Instant::now();
        let current = switch.english();
        assert!(!switch.shift_event_at(LEFT_SHIFT, true, start, current));
        assert!(!switch.shift_event_at(
            LEFT_SHIFT,
            false,
            start + Duration::from_millis(900),
            current
        ));
        assert!(!switch.english());
    }

    #[test]
    fn a_fixed_app_mode_wins_until_caps_lock_is_pressed() {
        // 没勾单击 Shift：模式本来由 Caps Lock 决定
        let mut switch = ModeSwitch::default();
        switch.force(Some(true), false);
        assert_eq!(
            switch.forced_mode(false),
            Some(true),
            "固定英文压过 Caps Lock"
        );
        // Caps Lock 被按过：这个应用里交回给 Caps Lock
        assert!(switch.release_forced(true));
        assert_eq!(switch.forced_mode(true), None);
        // 切走再切回：重新按名单固定
        switch.force(Some(false), true);
        assert_eq!(switch.forced_mode(true), Some(false));
        // 名单里没有的应用：清掉上一个应用的固定
        switch.force(None, true);
        assert_eq!(switch.forced_mode(true), None);
        // 没固定时什么都不放
        assert!(!switch.release_forced(false));
    }

    #[test]
    fn a_shift_click_takes_over_from_the_fixed_mode() {
        let mut switch = enabled();
        switch.force(Some(true), false);
        // 固定英文时单击一次就该到中文：从显示出来的模式翻，而不是从 english 字段
        let start = Instant::now();
        let current = switch.forced_mode(false).unwrap_or(switch.english());
        assert!(tap_at(&mut switch, start, current));
        assert!(!switch.english(), "切到了中文");
        assert_eq!(switch.forced_mode(false), None, "固定模式让位");
    }
}

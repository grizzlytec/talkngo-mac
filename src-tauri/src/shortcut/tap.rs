//! "Tap" semantics for a lone modifier key (e.g. Right Shift).
//!
//! handy-keys reports modifier-only hotkeys as pressed on key down, which would
//! also fire while typing capital letters. A tap is: the modifier goes down on
//! its own, nothing else happens (no other key, no other modifier), and it is
//! released within [`TAP_MAX`]. The detector is fed raw keyboard events from a
//! single stream, so ordering is exact.

use handy_keys::{Hotkey, KeyEvent, Modifiers};
use std::time::Duration;

/// Longest press that still counts as a tap.
pub const TAP_MAX: Duration = Duration::from_millis(400);

/// The single modifier of a lone-modifier hotkey string (e.g. `shift_right`),
/// or `None` for anything else (combos, keys, both-sides aliases like `shift`).
pub fn tap_modifier(hotkey_string: &str) -> Option<Modifiers> {
    let hotkey: Hotkey = hotkey_string.parse().ok()?;
    (hotkey.key.is_none() && hotkey.modifiers.bits().count_ones() == 1).then_some(hotkey.modifiers)
}

#[derive(Debug)]
pub struct TapDetector {
    target: Modifiers,
    down_at: Option<Duration>,
    tainted: bool,
}

impl TapDetector {
    pub fn new(target: Modifiers) -> Self {
        Self {
            target,
            down_at: None,
            tainted: false,
        }
    }

    /// Feeds one raw event with its timestamp (monotonic, from any origin).
    /// Returns true exactly when a tap just completed.
    pub fn feed(&mut self, event: &KeyEvent, at: Duration) -> bool {
        let is_target_change = event.key.is_none() && event.changed_modifier == Some(self.target);
        if !is_target_change {
            if self.down_at.is_some() {
                self.tainted = true;
            }
            return false;
        }
        if event.is_key_down {
            // Pressed alone only if no other modifier is held with it.
            self.down_at = Some(at);
            self.tainted = event.modifiers != self.target;
            return false;
        }
        let fire = match self.down_at.take() {
            Some(down) => {
                !self.tainted && event.modifiers.is_empty() && at.saturating_sub(down) < TAP_MAX
            }
            None => false,
        };
        self.tainted = false;
        fire
    }
}

/// Press length at which a hold starts push-to-talk. A press released earlier
/// is a tap, so taps are effectively shorter than this (<= [`TAP_MAX`]).
pub const HOLD_MIN: Duration = Duration::from_millis(300);
/// Longest gap between the first tap's release and the second tap's press.
pub const DOUBLE_TAP_GAP: Duration = Duration::from_millis(1200);

/// What the dictation state machine asks the coordinator to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DictationAction {
    /// Hold passed the threshold: start recording, stop on release.
    StartPushToTalk,
    StopPushToTalk,
    /// Double tap: start recording and keep it on.
    StartHandsFree,
    /// A clean tap while hands-free: stop recording.
    StopHandsFree,
}

/// Dictation on a lone modifier (e.g. Right Option):
/// hold = push-to-talk, double tap = hands-free (next clean tap stops it),
/// a single tap or any other key pressed together = nothing.
/// If another key arrives after push-to-talk already started, recording
/// continues (kept simple). Feed raw events with `feed` and call `tick`
/// regularly so the hold threshold can fire without an event.
#[derive(Debug)]
pub struct HoldTapMachine {
    target: Modifiers,
    down_at: Option<Duration>,
    tainted: bool,
    ptt_active: bool,
    hands_free: bool,
    last_tap_release: Option<Duration>,
}

impl HoldTapMachine {
    pub fn new(target: Modifiers) -> Self {
        Self {
            target,
            down_at: None,
            tainted: false,
            ptt_active: false,
            hands_free: false,
            last_tap_release: None,
        }
    }

    pub fn feed(&mut self, event: &KeyEvent, at: Duration) -> Option<DictationAction> {
        let is_target_change = event.key.is_none() && event.changed_modifier == Some(self.target);
        if !is_target_change {
            if self.down_at.is_some() && !self.ptt_active {
                self.tainted = true;
            }
            // Typing between the two taps breaks a double tap.
            self.last_tap_release = None;
            return None;
        }
        if event.is_key_down {
            self.down_at = Some(at);
            self.tainted = event.modifiers != self.target;
            self.ptt_active = false;
            return None;
        }
        let down = self.down_at.take()?;
        if self.ptt_active {
            self.ptt_active = false;
            self.last_tap_release = None;
            return Some(DictationAction::StopPushToTalk);
        }
        let clean = !self.tainted && event.modifiers.is_empty();
        self.tainted = false;
        if !clean {
            self.last_tap_release = None;
            return None;
        }
        if self.hands_free {
            self.hands_free = false;
            self.last_tap_release = None;
            return Some(DictationAction::StopHandsFree);
        }
        if at.saturating_sub(down) >= HOLD_MIN {
            // Held long enough but `tick` never ran: not a tap, not a hold.
            self.last_tap_release = None;
            return None;
        }
        match self.last_tap_release {
            Some(prev) if down.saturating_sub(prev) <= DOUBLE_TAP_GAP => {
                self.last_tap_release = None;
                self.hands_free = true;
                Some(DictationAction::StartHandsFree)
            }
            _ => {
                self.last_tap_release = Some(at);
                None
            }
        }
    }

    pub fn tick(&mut self, at: Duration) -> Option<DictationAction> {
        let down = self.down_at?;
        if !self.ptt_active
            && !self.tainted
            && !self.hands_free
            && at.saturating_sub(down) >= HOLD_MIN
        {
            self.ptt_active = true;
            self.last_tap_release = None;
            return Some(DictationAction::StartPushToTalk);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use handy_keys::Key;

    const RS: Modifiers = Modifiers::SHIFT_RIGHT;

    fn modifier(modifiers: Modifiers, down: bool, changed: Modifiers) -> KeyEvent {
        KeyEvent {
            modifiers,
            key: None,
            is_key_down: down,
            changed_modifier: Some(changed),
        }
    }

    fn key(modifiers: Modifiers, k: Key, down: bool) -> KeyEvent {
        KeyEvent {
            modifiers,
            key: Some(k),
            is_key_down: down,
            changed_modifier: None,
        }
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// Runs a sequence and returns the indexes at which a tap fired.
    fn run(events: &[(KeyEvent, u64)]) -> Vec<usize> {
        let mut d = TapDetector::new(RS);
        events
            .iter()
            .enumerate()
            .filter_map(|(i, (e, t))| d.feed(e, ms(*t)).then_some(i))
            .collect()
    }

    #[test]
    fn lone_tap_fires_on_release() {
        let seq = [
            (modifier(RS, true, RS), 0),
            (modifier(Modifiers::empty(), false, RS), 120),
        ];
        assert_eq!(run(&seq), vec![1]);
    }

    #[test]
    fn shift_plus_letter_does_not_fire() {
        let seq = [
            (modifier(RS, true, RS), 0),
            (key(RS, Key::A, true), 50),
            (key(RS, Key::A, false), 90),
            (modifier(Modifiers::empty(), false, RS), 120),
        ];
        assert!(run(&seq).is_empty());
    }

    #[test]
    fn long_hold_does_not_fire() {
        let seq = [
            (modifier(RS, true, RS), 0),
            (modifier(Modifiers::empty(), false, RS), 400),
        ];
        assert!(run(&seq).is_empty());
    }

    #[test]
    fn another_modifier_does_not_fire() {
        // Another modifier pressed during the tap.
        let during = [
            (modifier(RS, true, RS), 0),
            (
                modifier(RS | Modifiers::CMD_LEFT, true, Modifiers::CMD_LEFT),
                40,
            ),
            (modifier(RS, false, Modifiers::CMD_LEFT), 80),
            (modifier(Modifiers::empty(), false, RS), 120),
        ];
        assert!(run(&during).is_empty());
        // Another modifier already held when Right Shift goes down.
        let before = [
            (modifier(Modifiers::CMD_LEFT, true, Modifiers::CMD_LEFT), 0),
            (modifier(Modifiers::CMD_LEFT | RS, true, RS), 30),
            (modifier(Modifiers::CMD_LEFT, false, RS), 80),
        ];
        assert!(run(&before).is_empty());
    }

    #[test]
    fn left_shift_is_not_the_target() {
        let seq = [
            (
                modifier(Modifiers::SHIFT_LEFT, true, Modifiers::SHIFT_LEFT),
                0,
            ),
            (
                modifier(Modifiers::empty(), false, Modifiers::SHIFT_LEFT),
                80,
            ),
        ];
        assert!(run(&seq).is_empty());
    }

    #[test]
    fn consecutive_taps_each_fire_and_taint_resets() {
        let seq = [
            (modifier(RS, true, RS), 0),
            (key(RS, Key::B, true), 20),
            (modifier(Modifiers::empty(), false, RS), 60),
            (modifier(RS, true, RS), 200),
            (modifier(Modifiers::empty(), false, RS), 260),
        ];
        assert_eq!(run(&seq), vec![4]);
    }

    #[test]
    fn release_without_press_does_not_fire() {
        let seq = [(modifier(Modifiers::empty(), false, RS), 10)];
        assert!(run(&seq).is_empty());
    }

    #[test]
    fn tap_modifier_only_for_lone_single_modifiers() {
        assert_eq!(tap_modifier("shift_right"), Some(Modifiers::SHIFT_RIGHT));
        assert_eq!(tap_modifier("ctrl_right"), Some(Modifiers::CTRL_RIGHT));
        assert_eq!(tap_modifier("shift"), None);
        assert_eq!(tap_modifier("option+shift+r"), None);
        assert_eq!(tap_modifier("shift_right+option_left"), None);
        assert_eq!(tap_modifier("garbage!"), None);
    }

    mod dictation {
        use super::*;
        use DictationAction::*;

        /// Event or clock tick at a time.
        enum Step {
            Ev(KeyEvent, u64),
            Tick(u64),
        }
        use Step::*;

        fn run(steps: Vec<Step>) -> Vec<DictationAction> {
            let mut m = HoldTapMachine::new(Modifiers::OPT_RIGHT);
            steps
                .into_iter()
                .filter_map(|s| match s {
                    Ev(e, t) => m.feed(&e, ms(t)),
                    Tick(t) => m.tick(ms(t)),
                })
                .collect()
        }

        const RO: Modifiers = Modifiers::OPT_RIGHT;

        fn down(t: u64) -> Step {
            Ev(modifier(RO, true, RO), t)
        }
        fn up(t: u64) -> Step {
            Ev(modifier(Modifiers::empty(), false, RO), t)
        }

        #[test]
        fn hold_starts_at_threshold_and_stops_on_release() {
            let out = run(vec![down(0), Tick(100), Tick(300), Tick(400), up(900)]);
            assert_eq!(out, vec![StartPushToTalk, StopPushToTalk]);
        }

        #[test]
        fn double_tap_starts_hands_free_and_next_tap_stops() {
            let out = run(vec![
                down(0),
                up(100),
                down(500),
                up(600),
                down(5000),
                up(5100),
            ]);
            assert_eq!(out, vec![StartHandsFree, StopHandsFree]);
        }

        #[test]
        fn single_tap_does_nothing() {
            assert!(run(vec![down(0), Tick(50), up(100), Tick(2000)]).is_empty());
        }

        #[test]
        fn option_plus_key_does_nothing() {
            let out = run(vec![
                down(0),
                Ev(key(RO, Key::N, true), 100),
                Tick(400),
                Ev(key(RO, Key::N, false), 450),
                up(500),
            ]);
            assert!(out.is_empty());
        }

        #[test]
        fn slow_taps_do_nothing() {
            let out = run(vec![down(0), up(100), down(1400), up(1500)]);
            assert!(out.is_empty());
        }

        #[test]
        fn typing_between_taps_breaks_the_double_tap() {
            let out = run(vec![
                down(0),
                up(100),
                Ev(key(Modifiers::empty(), Key::A, true), 200),
                down(300),
                up(400),
            ]);
            assert!(out.is_empty());
        }

        #[test]
        fn other_key_after_push_to_talk_started_keeps_recording() {
            let out = run(vec![
                down(0),
                Tick(300),
                Ev(key(RO, Key::N, true), 500),
                up(900),
            ]);
            assert_eq!(out, vec![StartPushToTalk, StopPushToTalk]);
        }

        #[test]
        fn hold_after_a_tap_is_push_to_talk_not_hands_free() {
            let out = run(vec![down(0), up(100), down(400), Tick(700), up(1000)]);
            assert_eq!(out, vec![StartPushToTalk, StopPushToTalk]);
        }
    }
}

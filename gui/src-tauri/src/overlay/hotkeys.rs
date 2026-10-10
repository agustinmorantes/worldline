//! The overlay's hotkeys (handoff, "Hotkeys"): registered only while a tracked game with an install
//! dir is the window in front, so other apps keep those keys the rest of the time. Each entry in
//! `wanted` is an `(action, shortcut)`.
//!
//! The plugin runs every register and unregister on the main thread and waits for it, and calls
//! the handler there: never call `sync` or `check` on the main thread, and leave the handler at
//! once. Take `hotkeys` before `front`.
//!
//! A press is fixed in the handler (`Press`) and handled later on its own thread, by which time
//! the game in front, its run or the hotkeys may have changed. Each of those moves `Generation`
//! on, and a press from an earlier generation does nothing: it never acts for another game.

use super::decide::Front;
use super::naming::Naming;
use super::{lock, Overlay, OverlayConfig};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState};
use tauri_plugin_log::log::warn;

/// What a hotkey does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    QuickKeep,
    NameIt,
}

/// A hotkey: what it does and its keys.
pub type Entry = (Action, Shortcut);

/// A hotkey's presses within this time of its last accepted one are ignored.
const COOLDOWN: Duration = Duration::from_secs(2);

/// The shortcut a stored accelerator names, or `None` when it's off (`""`) or not one we accept.
/// The stored form is the plugin's: `Ctrl`, `Alt`, `Shift` in that order, then a key: A–Z, 0–9 or
/// F1–F24. F1–F24 may go alone; the rest need a modifier. F12 never: Windows keeps it for
/// debuggers.
pub fn parse(stored: &str) -> Option<Shortcut> {
    let mut tokens: Vec<&str> = stored.split('+').collect();
    let key = tokens.pop()?;
    let mut order = ["Ctrl", "Alt", "Shift"].into_iter();
    // Each modifier at most once, in order.
    if !tokens.iter().all(|token| order.any(|m| m == *token)) {
        return None;
    }
    let function = key
        .strip_prefix('F')
        .and_then(|n| n.parse::<u8>().ok())
        .filter(|n| (1..=24).contains(n) && key == format!("F{n}"));
    let plain = key.len() == 1
        && key
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit());
    let allowed = match function {
        Some(12) => false,
        Some(_) => true,
        None => plain && !tokens.is_empty(),
    };
    allowed.then(|| stored.parse().ok()).flatten()
}

/// The hotkeys `config` asks for, without the ones that are off or unreadable. Two actions on the
/// same keys (Settings refuses that) take neither.
pub fn wanted(config: &OverlayConfig) -> Vec<Entry> {
    let entries: Vec<Entry> = [
        (Action::QuickKeep, &config.quick_keep_key),
        (Action::NameIt, &config.name_it_key),
    ]
    .into_iter()
    .filter_map(|(action, stored)| parse(stored).map(|shortcut| (action, shortcut)))
    .collect();
    entries
        .iter()
        .filter(|(_, shortcut)| entries.iter().filter(|(_, s)| s == shortcut).count() == 1)
        .copied()
        .collect()
}

/// What a sync changes: the registered hotkeys to release, then the wanted ones to take.
fn plan(registered: &[Entry], wanted: &[Entry]) -> (Vec<Entry>, Vec<Entry>) {
    let release = registered
        .iter()
        .filter(|entry| !wanted.contains(entry))
        .copied()
        .collect();
    let take = wanted
        .iter()
        .filter(|entry| !registered.contains(entry))
        .copied()
        .collect();
    (release, take)
}

/// Numbers what the registered hotkeys act for: the game run in front and the hotkeys taken. Every
/// change, and every stop of a game, moves it on. Atomic, so the handler reads it on the main
/// thread without a lock.
#[derive(Debug, Default)]
pub struct Generation(AtomicU64);

impl Generation {
    pub fn current(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }

    pub fn advance(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

/// A press as the handler saw it: the hotkey, the generation it was made under and when.
#[derive(Debug, Clone, Copy)]
pub struct Press {
    pub action: Action,
    pub shortcut: Shortcut,
    pub generation: u64,
    pub at: Instant,
}

impl Press {
    /// Whether the press still stands: nothing in front, no run and no hotkey changed since it
    /// was made, and `config`, as it is now, still asks for its hotkey.
    pub fn stands(&self, generation: &Generation, config: &OverlayConfig) -> bool {
        self.generation == generation.current()
            && wanted(config).contains(&(self.action, self.shortcut))
    }
}

/// Records `next` as the game run in front, under the `front` lock; true, in a new generation, if
/// that changed.
pub fn set_front(front: &mut Option<Front>, next: Option<Front>, generation: &Generation) -> bool {
    if *front == next {
        return false;
    }
    *front = next;
    generation.advance();
    true
}

/// Each hotkey's last accepted press.
#[derive(Debug, Default)]
pub struct Cooldown(HashMap<Action, Instant>);

impl Cooldown {
    /// True if a press of `action` at `now` counts: its first, or `COOLDOWN` after the last one
    /// that counted. Ignored presses don't restart the wait.
    pub fn accept(&mut self, action: Action, now: Instant) -> bool {
        let ready = self
            .0
            .get(&action)
            .is_none_or(|last| now.saturating_duration_since(*last) >= COOLDOWN);
        if ready {
            self.0.insert(action, now);
        }
        ready
    }
}

/// Whether a Name it press at `at` counts: never while a card is kept for or open, which doesn't
/// start its cooldown either, nor within its cooldown. One that counts claims the card.
pub fn accept_name_it(naming: &mut Naming, cooldown: &mut Cooldown, at: Instant) -> bool {
    if !naming.begin() {
        return false;
    }
    if cooldown.accept(Action::NameIt, at) {
        return true;
    }
    naming.end();
    false
}

/// Registers the wanted hotkeys while a game is in front and the GUI has pushed its config, and
/// unregisters the rest. A shortcut another app holds stays unregistered until the next sync.
pub fn sync(app: &AppHandle) {
    let overlay = app.state::<Overlay>();
    let mut registered = lock(&overlay.hotkeys);
    let in_front = lock(&overlay.front).is_some();
    let wanted = match lock(&overlay.config).as_ref() {
        Some(config) if in_front => wanted(config),
        _ => Vec::new(),
    };
    let (release, take) = plan(&registered, &wanted);
    let shortcuts = app.global_shortcut();
    for entry in release.iter().copied() {
        if let Err(err) = shortcuts.unregister(entry.1) {
            warn!("overlay: couldn't release the hotkey {}: {err}", entry.1);
        }
        registered.retain(|kept| *kept != entry);
    }
    // Presses of the released hotkeys still on their way are stale from here.
    if !release.is_empty() || !take.is_empty() {
        overlay.generation.advance();
    }
    for (action, shortcut) in take {
        let handler = move |app: &AppHandle, _: &Shortcut, event: ShortcutEvent| {
            // A held key is one press. On the main thread: fix it and hand it off at once.
            if event.state == ShortcutState::Pressed {
                let press = Press {
                    action,
                    shortcut,
                    generation: app.state::<Overlay>().generation.current(),
                    at: Instant::now(),
                };
                let app = app.clone();
                std::thread::spawn(move || super::pressed(&app, press));
            }
        };
        match shortcuts.on_shortcut(shortcut, handler) {
            Ok(()) => registered.push((action, shortcut)),
            Err(err) => warn!("overlay: couldn't take the hotkey {shortcut}: {err}"),
        }
    }
}

/// Whether `stored` can be registered now: it's one of ours already, or registering and releasing
/// it works. Under the registry lock, so a sync can't take or lose it in between.
pub fn check(app: &AppHandle, stored: &str) -> Result<(), String> {
    let shortcut = parse(stored).ok_or_else(|| format!("Not a shortcut: {stored}"))?;
    let overlay = app.state::<Overlay>();
    let registered = lock(&overlay.hotkeys);
    if registered.iter().any(|(_, taken)| *taken == shortcut) {
        return Ok(());
    }
    let shortcuts = app.global_shortcut();
    shortcuts.register(shortcut).map_err(|e| e.to_string())?;
    shortcuts.unregister(shortcut).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::super::decide::Games;
    use super::*;
    use tauri_plugin_global_shortcut::{Code, Modifiers};

    fn shortcut(mods: Modifiers, key: Code) -> Option<Shortcut> {
        Some(Shortcut::new(Some(mods), key))
    }

    #[test]
    fn reads_the_stored_accelerators_it_accepts() {
        let both = Modifiers::CONTROL | Modifiers::SHIFT;
        assert_eq!(parse("Ctrl+Shift+K"), shortcut(both, Code::KeyK));
        assert_eq!(parse("Alt+7"), shortcut(Modifiers::ALT, Code::Digit7));
        assert_eq!(
            parse("Ctrl+Alt+Shift+F24"),
            shortcut(both | Modifiers::ALT, Code::F24)
        );
        // F1–F24 may go alone.
        assert_eq!(parse("F1"), shortcut(Modifiers::empty(), Code::F1));
        assert_eq!(parse("F13"), shortcut(Modifiers::empty(), Code::F13));
        // The same shortcut, so the registry and the plugin agree on it.
        assert_eq!(parse("Ctrl+Shift+K"), "ctrl+shift+KeyK".parse().ok());
    }

    #[test]
    fn refuses_lone_modifiers_f12_bare_keys_and_anything_else() {
        for stored in [
            "",
            "Ctrl",
            "Ctrl+Shift",
            "Shift+",
            "+K",
            "K",
            "7",
            "F12",
            "Ctrl+F12",
            "F0",
            "F25",
            "F01",
            "Shift+Ctrl+K",
            "Ctrl+Ctrl+K",
            "ctrl+shift+k",
            "Ctrl+Shift+k",
            "Ctrl+KeyK",
            "Ctrl+Space",
            "Super+K",
            "Ctrl+K+J",
            "Ctrl+Shift+Enter",
        ] {
            assert_eq!(parse(stored), None, "{stored:?}");
        }
    }

    fn config(quick_keep_key: &str) -> OverlayConfig {
        both(quick_keep_key, "")
    }

    fn both(quick_keep_key: &str, name_it_key: &str) -> OverlayConfig {
        serde_json::from_value(serde_json::json!({
            "enabled": false, "sound": true, "corner": "br", "perGame": {}, "quiet": false,
            "warnAhead": true, "quickKeepKey": quick_keep_key, "nameItKey": name_it_key,
            "keptTagTemplate": "Kept in game, {time}", "motion": "full", "language": "en",
        }))
        .unwrap()
    }

    const KEEP: Action = Action::QuickKeep;
    const NAME: Action = Action::NameIt;

    #[test]
    fn the_pushed_config_names_the_hotkeys_even_with_the_popup_off() {
        let k = parse("Ctrl+Shift+K").unwrap();
        let n = parse("Ctrl+Shift+N").unwrap();
        assert_eq!(wanted(&config("Ctrl+Shift+K")), [(KEEP, k)]);
        assert_eq!(
            wanted(&both("Ctrl+Shift+K", "Ctrl+Shift+N")),
            [(KEEP, k), (NAME, n)]
        );
        assert_eq!(wanted(&both("", "Ctrl+Shift+N")), [(NAME, n)]);
        // Turned off, or a value we don't accept: nothing to register.
        assert_eq!(wanted(&config("")), []);
        assert_eq!(wanted(&config("Ctrl+Shift")), []);
    }

    #[test]
    fn two_actions_on_the_same_keys_take_neither() {
        // Settings refuses it; a hand-edited prefs file can still say it.
        assert_eq!(wanted(&both("Ctrl+Shift+K", "Ctrl+Shift+K")), []);
    }

    #[test]
    fn each_hotkey_has_its_own_cooldown() {
        let mut cooldown = Cooldown::default();
        let now = Instant::now();
        assert!(cooldown.accept(KEEP, now));
        assert!(cooldown.accept(NAME, now + Duration::from_millis(100)));
        assert!(!cooldown.accept(NAME, now + Duration::from_millis(500)));
    }

    #[test]
    fn name_it_is_ignored_while_its_card_is_kept_for_or_open_without_starting_its_cooldown() {
        use super::super::naming::{Opening, Place};
        let mut naming = Naming::default();
        let mut cooldown = Cooldown::default();
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        assert!(accept_name_it(&mut naming, &mut cooldown, at(0)));
        // Its keep runs: a second press, even past the cooldown, does nothing.
        assert!(!accept_name_it(&mut naming, &mut cooldown, at(2500)));
        let opening = Opening {
            game_id: 7,
            version_id: 40,
            version_number: 12,
            auto: "Auto".into(),
            game_window: 1,
            place: Place::App,
            was_tray: false,
        };
        let id = naming.open(opening).unwrap().id;
        assert!(!accept_name_it(&mut naming, &mut cooldown, at(3000)));
        naming.close(id, Place::App, super::super::naming::How::Save);
        // Closed: the ignored presses didn't restart the cooldown, so this one counts.
        assert!(accept_name_it(&mut naming, &mut cooldown, at(3100)));
        naming.end();
        // Within the cooldown of that one: ignored, and the card stays free.
        assert!(!accept_name_it(&mut naming, &mut cooldown, at(3200)));
        assert!(naming.begin());
    }

    #[test]
    fn a_name_it_press_waiting_while_its_hotkey_is_rebound_is_stale() {
        let mut games = Games::default();
        let (_, generation, mut press) = pressed_in(&mut games);
        press.action = NAME;
        press.shortcut = parse("Ctrl+Shift+N").unwrap();
        assert!(press.stands(&generation, &both("Ctrl+Shift+K", "Ctrl+Shift+N")));
        assert!(!press.stands(&generation, &both("Ctrl+Shift+K", "Alt+N")));
        assert!(!press.stands(&generation, &both("Ctrl+Shift+N", "Ctrl+Shift+N")));
    }

    #[test]
    fn a_sync_releases_what_isnt_wanted_and_takes_what_is() {
        let k = (KEEP, parse("Ctrl+Shift+K").unwrap());
        let j = (KEEP, parse("Alt+J").unwrap());
        // The game comes to the front, and leaves it.
        assert_eq!(plan(&[], &[k]), (vec![], vec![k]));
        assert_eq!(plan(&[k], &[]), (vec![k], vec![]));
        // Nothing changed: the plugin isn't called.
        assert_eq!(plan(&[k], &[k]), (vec![], vec![]));
        // A new shortcut in the config replaces the old one.
        assert_eq!(plan(&[k], &[j]), (vec![k], vec![j]));
    }

    /// Game 7's run in front, its hotkey registered, and a press of it waiting for its thread.
    fn pressed_in(games: &mut Games) -> (Option<Front>, Generation, Press) {
        games.started(7, Some(r"C:\Games\Seven"));
        games.started(8, Some(r"C:\Games\Eight"));
        let generation = Generation::default();
        let mut front = None;
        assert!(set_front(
            &mut front,
            Some((7, games.run(7).unwrap())),
            &generation
        ));
        let press = Press {
            action: KEEP,
            shortcut: parse("Ctrl+Shift+K").unwrap(),
            generation: generation.current(),
            at: Instant::now(),
        };
        (front, generation, press)
    }

    #[test]
    fn a_press_stands_while_nothing_changed() {
        let mut games = Games::default();
        let (mut front, generation, press) = pressed_in(&mut games);
        // The same run in front again isn't a change.
        assert!(!set_front(
            &mut front,
            Some((7, games.run(7).unwrap())),
            &generation
        ));
        assert!(press.stands(&generation, &config("Ctrl+Shift+K")));
    }

    #[test]
    fn a_press_waiting_while_another_game_comes_to_the_front_is_stale() {
        let mut games = Games::default();
        let (mut front, generation, press) = pressed_in(&mut games);
        assert!(set_front(
            &mut front,
            Some((8, games.run(8).unwrap())),
            &generation
        ));
        assert!(!press.stands(&generation, &config("Ctrl+Shift+K")));
        // Game 7 back in front doesn't revive it.
        set_front(&mut front, Some((7, games.run(7).unwrap())), &generation);
        assert!(!press.stands(&generation, &config("Ctrl+Shift+K")));
    }

    #[test]
    fn a_press_waiting_while_the_game_stops_and_starts_again_is_stale() {
        let mut games = Games::default();
        let (mut front, generation, press) = pressed_in(&mut games);
        // `game_stopped` moves the generation on with the stop, then the front follows.
        games.stopped(7);
        generation.advance();
        games.started(7, Some(r"C:\Games\Seven"));
        assert!(set_front(
            &mut front,
            Some((7, games.run(7).unwrap())),
            &generation
        ));
        assert!(!press.stands(&generation, &config("Ctrl+Shift+K")));
    }

    #[test]
    fn a_press_waiting_while_its_hotkey_is_turned_off_or_rebound_is_stale() {
        let mut games = Games::default();
        let (_, generation, press) = pressed_in(&mut games);
        // The new config is in before the sync that releases the hotkey.
        assert!(!press.stands(&generation, &config("")));
        assert!(!press.stands(&generation, &config("Alt+J")));
        // And once the sync released it, even back to the same keys.
        generation.advance();
        assert!(!press.stands(&generation, &config("Ctrl+Shift+K")));
    }

    #[test]
    fn ten_presses_in_two_seconds_count_once_and_the_next_after_them_counts() {
        let mut cooldown = Cooldown::default();
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        let counted = (0..10)
            .filter(|i| cooldown.accept(Action::QuickKeep, at(i * 199)))
            .count();
        assert_eq!(counted, 1);
        // Counted from the accepted press, not the ignored ones.
        assert!(cooldown.accept(Action::QuickKeep, at(2000)));
        assert!(!cooldown.accept(Action::QuickKeep, at(3999)));
        assert!(cooldown.accept(Action::QuickKeep, at(4000)));
    }
}

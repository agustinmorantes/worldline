//! The overlay's decisions, free of Win32 so they're tested on every platform: the corner, where
//! the window goes, what a new event does to the popup on screen, which sound goes with it, and
//! whether a session starts with the device-ahead warning.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use uuid::Uuid;
use worldline_core::sync::{Divergence, GameSyncStatus, Parents};

/// A corner of the game's client area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Corner {
    Br,
    Bl,
    Tr,
    Tl,
}

/// One game's choice (Game menu › In-game popup): a corner, or no popups while it plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GameCorner {
    Br,
    Bl,
    Tr,
    Tl,
    Off,
}

/// The corner for a game's popups, or `None` when it gets none: the popup is off, or off for
/// this game. A game's own corner wins over the default.
pub fn corner(enabled: bool, default: Corner, game: Option<GameCorner>) -> Option<Corner> {
    if !enabled {
        return None;
    }
    match game {
        None => Some(default),
        Some(GameCorner::Br) => Some(Corner::Br),
        Some(GameCorner::Bl) => Some(Corner::Bl),
        Some(GameCorner::Tr) => Some(Corner::Tr),
        Some(GameCorner::Tl) => Some(Corner::Tl),
        Some(GameCorner::Off) => None,
    }
}

/// A screen rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// The largest popup, in logical px: the 48 px tall pill at its widest (handoff, "Anatomy").
pub const POPUP: (f64, f64) = (300.0, 48.0);
/// The popup's distance from the client area's edges (handoff, "Placement").
const INSET: f64 = 16.0;
/// Transparent room on every side of the popup, so the rise's overshoot and the halo aren't cut
/// off. The webview pads the popup by the same amount.
pub const MARGIN: f64 = 16.0;

/// The overlay window's rectangle for a `popup` in `client`'s `corner`, all scaled by the
/// monitor's `scale`: the popup sits `INSET` from the client edges, with `MARGIN` around it.
pub fn place(client: Rect, popup: (f64, f64), scale: f64, corner: Corner) -> Rect {
    let w = ((popup.0 + 2.0 * MARGIN) * scale).round() as i32;
    let h = ((popup.1 + 2.0 * MARGIN) * scale).round() as i32;
    let inset = ((INSET - MARGIN) * scale).round() as i32;
    let x = match corner {
        Corner::Br | Corner::Tr => client.x + client.w - inset - w,
        Corner::Bl | Corner::Tl => client.x + inset,
    };
    let y = match corner {
        Corner::Br | Corner::Bl => client.y + client.h - inset - h,
        Corner::Tr | Corner::Tl => client.y + inset,
    };
    Rect { x, y, w, h }
}

/// What a popup is about (handoff, "Events").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Checkpoint,
    SessionStart,
    NewTimeline,
    /// The Quick keep and Name it hotkeys (O3, O4) trigger it.
    Kept,
    /// A warning.
    DeviceAhead,
    /// An error.
    CaptureFailed,
}

impl Kind {
    /// Warnings and errors, which nothing else replaces (handoff, "Rules"). They're about the
    /// user's data, so quiet mode shows them in full.
    pub fn urgent(self) -> bool {
        matches!(self, Kind::DeviceAhead | Kind::CaptureFailed)
    }

    /// How long the popup stays before it folds away. Quiet mode shows only the mark, briefly.
    pub fn duration(self, quiet: bool) -> Duration {
        let millis = match self {
            Kind::Checkpoint | Kind::NewTimeline | Kind::Kept if quiet => 1600,
            Kind::Checkpoint | Kind::Kept => 3200,
            Kind::SessionStart => 4200,
            Kind::NewTimeline => 4600,
            Kind::DeviceAhead => 8000,
            Kind::CaptureFailed => 6000,
        };
        Duration::from_millis(millis)
    }
}

/// What an incoming event does to the popup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing is showing: the popup rises.
    Show,
    /// A popup is showing: its content changes in place and its timer restarts. Never stacked.
    Swap,
    /// Dropped, not queued: a warning or error is showing, or it came too late.
    Drop,
    /// Quiet mode leaves it out.
    Skip,
}

/// `incoming`'s outcome while `showing` is on screen. A warning or error drops everything below
/// it; any other event swaps in, another warning or error too (a capture that fails again, maybe
/// for another reason, restarts the timer with its own text).
pub fn outcome(showing: Option<Kind>, incoming: Kind, quiet: bool) -> Outcome {
    if quiet && incoming == Kind::SessionStart {
        return Outcome::Skip;
    }
    match showing {
        None => Outcome::Show,
        Some(current) if current.urgent() && !incoming.urgent() => Outcome::Drop,
        Some(_) => Outcome::Swap,
    }
}

/// One marimba sound per event (handoff, "Sound"). Session start has none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sound {
    Checkpoint,
    Kept,
    NewTimeline,
    DeviceAhead,
    CaptureFailed,
}

/// The sound that goes with `outcome` for `kind`: the incoming event's sound on a swap, nothing
/// when it's dropped or skipped, or for a session start.
pub fn sound(kind: Kind, outcome: Outcome) -> Option<Sound> {
    match outcome {
        Outcome::Drop | Outcome::Skip => None,
        Outcome::Show | Outcome::Swap => match kind {
            Kind::Checkpoint => Some(Sound::Checkpoint),
            Kind::SessionStart => None,
            Kind::Kept => Some(Sound::Kept),
            Kind::NewTimeline => Some(Sound::NewTimeline),
            Kind::DeviceAhead => Some(Sound::DeviceAhead),
            Kind::CaptureFailed => Some(Sound::CaptureFailed),
        },
    }
}

/// What became of an event offered to the popup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presented {
    /// It showed or swapped in, with its sound.
    Shown,
    /// The popup turned it away (`Outcome::Drop` or `Skip`): it plays nothing.
    Dropped,
    /// No popup could show at all: off, globally or for the game, the game not in front, or no
    /// window ready.
    Unavailable,
}

impl From<Outcome> for Presented {
    fn from(outcome: Outcome) -> Self {
        match outcome {
            Outcome::Show | Outcome::Swap => Presented::Shown,
            Outcome::Drop | Outcome::Skip => Presented::Dropped,
        }
    }
}

/// The sound a hotkey still owes the player once its result was `presented`: its kind's, once,
/// only when no popup could show and `sound` (the setting as it is now) is on. A popup that showed
/// played its own; a dropped one stays silent.
pub fn hotkey_sound(kind: Kind, presented: Presented, sound_on: bool) -> Option<Sound> {
    match presented {
        Presented::Unavailable if sound_on => sound(kind, Outcome::Show),
        _ => None,
    }
}

/// The popup on screen. Every show or swap gets a new generation, and the window's delayed work
/// (its timer, the hide after the exit animation) only acts while its generation is current.
#[derive(Debug, Default)]
pub struct Popup {
    generation: u64,
    phase: Phase,
    /// The overlay webview has loaded and listens for popups.
    ready: bool,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Phase {
    #[default]
    Hidden,
    Showing(Kind),
    /// Its exit animation runs; the window hides when it ends. `fast` once it's the quick fade.
    Leaving {
        fast: bool,
    },
}

/// What a popup's watcher does after one look at the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Watch {
    /// Keep it above the game and look again.
    Keep,
    /// Ask the webview to play the exit: the fold, or the quick fade when `fast`.
    Leave { fast: bool },
    /// Hide the window: its exit never said it ended.
    Hide,
    /// Nothing left to watch: a newer popup took over, or the exit ended.
    Stop,
}

impl Popup {
    /// Takes `incoming`, and returns its outcome with the popup's generation. Dropped while the
    /// webview isn't ready. A popup that is leaving is gone: the next one rises again.
    pub fn offer(&mut self, incoming: Kind, quiet: bool) -> (Outcome, u64) {
        let showing = match self.phase {
            Phase::Showing(kind) => Some(kind),
            Phase::Hidden | Phase::Leaving { .. } => None,
        };
        let outcome = if self.ready {
            outcome(showing, incoming, quiet)
        } else {
            Outcome::Drop
        };
        if matches!(outcome, Outcome::Show | Outcome::Swap) {
            self.generation += 1;
            self.phase = Phase::Showing(incoming);
        }
        (outcome, self.generation)
    }

    /// One look at popup `generation` by its watcher, from its show until the window hides.
    /// `away`: the game left the front, moved, resized or minimised. `due`: its time is up.
    /// `late`: its exit started more than the fallback limit ago. Away during the fold still
    /// switches to the quick fade, once.
    pub fn watch(&mut self, generation: u64, away: bool, due: bool, late: bool) -> Watch {
        if self.generation != generation {
            return Watch::Stop;
        }
        match self.phase {
            Phase::Hidden => Watch::Stop,
            Phase::Showing(_) if away || due => {
                self.phase = Phase::Leaving { fast: away };
                Watch::Leave { fast: away }
            }
            Phase::Showing(_) => Watch::Keep,
            Phase::Leaving { .. } if late => {
                self.phase = Phase::Hidden;
                Watch::Hide
            }
            Phase::Leaving { fast: false } if away => {
                self.phase = Phase::Leaving { fast: true };
                Watch::Leave { fast: true }
            }
            Phase::Leaving { .. } => Watch::Keep,
        }
    }

    /// Ends `generation`'s exit; true if the window should hide now. False when a newer popup
    /// took over meanwhile, or it already ended.
    pub fn finish(&mut self, generation: u64) -> bool {
        let leaving = self.generation == generation && matches!(self.phase, Phase::Leaving { .. });
        if leaving {
            self.phase = Phase::Hidden;
        }
        leaving
    }

    /// The overlay webview listens for popups.
    pub fn is_ready(&self) -> bool {
        self.ready
    }

    pub fn set_ready(&mut self) {
        self.ready = true;
    }

    /// The window is being created or destroyed: everything in flight is stale, and nothing
    /// shows until the new webview is ready.
    pub fn reset(&mut self) {
        self.generation += 1;
        self.phase = Phase::Hidden;
        self.ready = false;
    }
}

/// A running game and its run: the one in front, for the hotkeys.
pub type Front = (i32, u64);

/// The running games that can get a popup: tracked, with an install dir. The overlay window
/// exists while there is one. Each run of a game is armed for one session start.
#[derive(Debug, Default)]
pub struct Games {
    running: HashMap<i32, Run>,
    /// The last run handed out; every `GameStarted` gets a new one.
    runs: u64,
}

#[derive(Debug)]
struct Run {
    install_dir: String,
    id: u64,
    /// Its session start hasn't been taken yet.
    armed: bool,
}

impl Games {
    /// Records a started game and arms its session start; true if it counts (it has an install
    /// dir).
    pub fn started(&mut self, game_id: i32, install_dir: Option<&str>) -> bool {
        let Some(install_dir) = install_dir else {
            return false;
        };
        self.runs += 1;
        let run = Run {
            install_dir: install_dir.to_owned(),
            id: self.runs,
            armed: true,
        };
        self.running.insert(game_id, run);
        true
    }

    /// Records a stopped game, which disarms it; true if it was the last one that counted.
    pub fn stopped(&mut self, game_id: i32) -> bool {
        self.running.remove(&game_id).is_some() && self.running.is_empty()
    }

    /// The running game whose install dir `exe` runs from, if any.
    pub fn running_from(&self, exe: &str) -> Option<i32> {
        let exe = std::path::Path::new(exe);
        self.running
            .iter()
            .find(|(_, run)| worldline_core::detection::runs_from(exe, &run.install_dir))
            .map(|(game_id, _)| *game_id)
    }

    /// The game came to the front: its run, the first time only (one session start per
    /// `GameStarted`).
    pub fn take_session(&mut self, game_id: i32) -> Option<u64> {
        let run = self.running.get_mut(&game_id)?;
        std::mem::take(&mut run.armed).then_some(run.id)
    }

    /// The game's current run, if it's running.
    pub fn run(&self, game_id: i32) -> Option<u64> {
        self.running.get(&game_id).map(|run| run.id)
    }

    /// The install dir of the game's run `run`, or `None` once that run has stopped.
    pub fn install_dir(&self, game_id: i32, run: u64) -> Option<&str> {
        self.running
            .get(&game_id)
            .filter(|r| r.id == run)
            .map(|r| r.install_dir.as_str())
    }
}

/// Offers `kind` to the popup, unless it's the session start of game `run.0`'s run `run.1` and
/// that run has ended: dropped then, its delayed work is stale. The caller holds the lifecycle
/// lock (`games`) from here until the popup shows, so a stop or a restart can't slip in between.
pub fn admit(
    games: &Games,
    popup: &mut Popup,
    kind: Kind,
    run: Option<(i32, u64)>,
    quiet: bool,
) -> (Outcome, u64) {
    if run.is_some_and(|(game_id, run)| games.install_dir(game_id, run).is_none()) {
        return (Outcome::Drop, popup.generation);
    }
    popup.offer(kind, quiet)
}

/// Another device that is ahead of this one on the game's line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ahead {
    pub device: String,
    /// Its versions newer than this device's.
    pub count: u32,
}

/// The device-ahead warning that replaces a session start, or `None` for the plain session start:
/// the warning or cloud sync is off, the status is unknown, the relation isn't `fast_forward`,
/// sync already applied it, or the device or the count can't be told. `status` and `parents`
/// are the game's sync status now and its versions' parents on the remote
/// (`WorldlineCore::game_sync_status`); `local` is its active version.
pub fn device_ahead(
    warn_ahead: bool,
    cloud_on: bool,
    status: Option<&GameSyncStatus>,
    parents: &Parents,
    local: Option<Uuid>,
) -> Option<Ahead> {
    if !warn_ahead || !cloud_on {
        return None;
    }
    let status = status?;
    if status.relation != Divergence::FastForward || status.auto_applied {
        return None;
    }
    let device = status.remote_device.clone()?;
    let count = newer(parents, status.remote_version_uuid?, local?)?;
    Some(Ahead { device, count })
}

/// How many versions `tip` is ahead of its ancestor `local`, following `parents`. `None` if
/// `local` isn't an ancestor.
fn newer(parents: &Parents, tip: Uuid, local: Uuid) -> Option<u32> {
    let mut at = tip;
    let mut count = 0;
    while at != local {
        at = (*parents.get(&at)?)?;
        count += 1;
        // Like `sync::is_ancestor`: a broken history can't loop forever.
        if count as usize > parents.len() {
            return None;
        }
    }
    (count > 0).then_some(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_games_own_corner_wins_and_off_silences_it() {
        assert_eq!(corner(true, Corner::Br, None), Some(Corner::Br));
        assert_eq!(
            corner(true, Corner::Br, Some(GameCorner::Tl)),
            Some(Corner::Tl)
        );
        assert_eq!(corner(true, Corner::Br, Some(GameCorner::Off)), None);
        // The global switch silences every game, whatever its own choice.
        assert_eq!(corner(false, Corner::Br, None), None);
        assert_eq!(corner(false, Corner::Br, Some(GameCorner::Tr)), None);
    }

    #[test]
    fn per_game_choices_parse_from_the_pushed_config() {
        let parsed: Vec<GameCorner> =
            serde_json::from_str(r#"["br","bl","tr","tl","off"]"#).unwrap();
        assert_eq!(
            parsed,
            [
                GameCorner::Br,
                GameCorner::Bl,
                GameCorner::Tr,
                GameCorner::Tl,
                GameCorner::Off
            ]
        );
        assert!(serde_json::from_str::<GameCorner>(r#""centre""#).is_err());
    }

    const CLIENT: Rect = Rect {
        x: 100,
        y: 50,
        w: 1920,
        h: 1080,
    };

    /// The popup's own rectangle inside the window `place` returns.
    fn popup_in(window: Rect, scale: f64) -> Rect {
        let margin = (MARGIN * scale).round() as i32;
        Rect {
            x: window.x + margin,
            y: window.y + margin,
            w: window.w - 2 * margin,
            h: window.h - 2 * margin,
        }
    }

    #[test]
    fn places_the_popup_in_each_corner_16_px_inside_the_client_area() {
        let at = |corner| popup_in(place(CLIENT, POPUP, 1.0, corner), 1.0);
        let (w, h) = (300, 48);
        assert_eq!(
            at(Corner::Br),
            Rect {
                x: 100 + 1920 - 16 - w,
                y: 50 + 1080 - 16 - h,
                w,
                h
            }
        );
        assert_eq!(
            at(Corner::Bl),
            Rect {
                x: 116,
                y: 50 + 1080 - 16 - h,
                w,
                h
            }
        );
        assert_eq!(
            at(Corner::Tr),
            Rect {
                x: 100 + 1920 - 16 - w,
                y: 66,
                w,
                h
            }
        );
        assert_eq!(
            at(Corner::Tl),
            Rect {
                x: 116,
                y: 66,
                w,
                h
            }
        );
    }

    #[test]
    fn scales_the_popup_and_its_inset_at_150_percent() {
        let at = |corner| popup_in(place(CLIENT, POPUP, 1.5, corner), 1.5);
        let (w, h) = (450, 72);
        assert_eq!(
            at(Corner::Br),
            Rect {
                x: 100 + 1920 - 24 - w,
                y: 50 + 1080 - 24 - h,
                w,
                h
            }
        );
        assert_eq!(
            at(Corner::Bl),
            Rect {
                x: 124,
                y: 50 + 1080 - 24 - h,
                w,
                h
            }
        );
        assert_eq!(
            at(Corner::Tr),
            Rect {
                x: 100 + 1920 - 24 - w,
                y: 74,
                w,
                h
            }
        );
        assert_eq!(
            at(Corner::Tl),
            Rect {
                x: 124,
                y: 74,
                w,
                h
            }
        );
    }

    #[test]
    fn the_window_stays_inside_the_client_area() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            for corner in [Corner::Br, Corner::Bl, Corner::Tr, Corner::Tl] {
                let r = place(CLIENT, POPUP, scale, corner);
                assert!(r.x >= CLIENT.x && r.y >= CLIENT.y, "{corner:?} at {scale}");
                assert!(r.x + r.w <= CLIENT.x + CLIENT.w, "{corner:?} at {scale}");
                assert!(r.y + r.h <= CLIENT.y + CLIENT.h, "{corner:?} at {scale}");
            }
        }
    }

    use Kind::{
        CaptureFailed as Fail, Checkpoint as Cp, DeviceAhead as Warn, Kept, NewTimeline as Tl,
        SessionStart as Ss,
    };
    use Outcome::{Drop as D, Show as Sh, Skip as Sk, Swap as Sw};

    const KINDS: [Kind; 6] = [Cp, Ss, Tl, Kept, Warn, Fail];
    const CP: Kind = Cp;

    /// Every (showing, incoming) pair, against the handoff's "Rules": one popup that swaps, never
    /// stacks; warnings and errors drop the rest, and replace each other and themselves.
    #[test]
    fn every_event_against_every_popup_on_screen() {
        // Columns: incoming in `KINDS` order. Rows: what shows.
        let table: [(Option<Kind>, [Outcome; 6]); 7] = [
            (None, [Sh, Sh, Sh, Sh, Sh, Sh]),
            (Some(Cp), [Sw, Sw, Sw, Sw, Sw, Sw]),
            (Some(Ss), [Sw, Sw, Sw, Sw, Sw, Sw]),
            (Some(Tl), [Sw, Sw, Sw, Sw, Sw, Sw]),
            (Some(Kept), [Sw, Sw, Sw, Sw, Sw, Sw]),
            (Some(Warn), [D, D, D, D, Sw, Sw]),
            (Some(Fail), [D, D, D, D, Sw, Sw]),
        ];
        for (showing, row) in table {
            for (incoming, expected) in KINDS.into_iter().zip(row) {
                let got = outcome(showing, incoming, false);
                assert_eq!(got, expected, "{incoming:?} while {showing:?} shows");
            }
        }
    }

    /// A capture fails, and its retry two seconds later fails again for another reason: the new
    /// error takes over the popup (its text is the swap's payload), restarts the timer and plays
    /// its sound again. Lower events still drop.
    #[test]
    fn a_failure_after_a_failure_swaps_in_and_restarts_the_timer() {
        let mut popup = ready();
        let (outcome, first) = popup.offer(Fail, false);
        assert_eq!(
            (outcome, sound(Fail, outcome)),
            (Sh, Some(Sound::CaptureFailed))
        );
        let (outcome, second) = popup.offer(Fail, false);
        assert_eq!(
            (outcome, sound(Fail, outcome)),
            (Sw, Some(Sound::CaptureFailed))
        );
        assert_ne!(first, second, "a new generation");
        // The first failure's timer no longer ends the popup; the second's runs from the start.
        assert_eq!(look(&mut popup, first, DUE), Watch::Stop);
        assert_eq!(look(&mut popup, second, STILL), Watch::Keep);
        let (outcome, third) = popup.offer(Cp, false);
        assert_eq!((outcome, third), (D, second));
        assert_eq!(popup.offer(Warn, false).0, Sw);
        assert_eq!(popup.offer(Fail, false).0, Sw);
    }

    #[test]
    fn quiet_mode_skips_only_the_session_start() {
        for showing in [None, Some(Cp), Some(Warn), Some(Fail)] {
            assert_eq!(outcome(showing, Ss, true), Sk, "while {showing:?}");
        }
        // Everything else is decided as without quiet mode.
        for incoming in [Cp, Tl, Kept, Warn, Fail] {
            for showing in [None, Some(Cp), Some(Warn)] {
                assert_eq!(
                    outcome(showing, incoming, true),
                    outcome(showing, incoming, false),
                    "{incoming:?} while {showing:?}"
                );
            }
        }
    }

    #[test]
    fn quiet_mode_shows_the_mark_briefly_but_warnings_and_errors_in_full() {
        let ms = |kind: Kind, quiet| kind.duration(quiet).as_millis();
        let full: Vec<u128> = KINDS.iter().map(|&k| ms(k, false)).collect();
        assert_eq!(full, [3200, 4200, 4600, 3200, 8000, 6000]);
        for kind in [Cp, Tl, Kept] {
            assert_eq!(ms(kind, true), 1600, "{kind:?}");
        }
        assert_eq!(ms(Warn, true), 8000);
        assert_eq!(ms(Fail, true), 6000);
    }

    #[test]
    fn each_shown_or_swapped_event_plays_its_sound_and_a_drop_or_skip_nothing() {
        let expected = [
            (Cp, Some(Sound::Checkpoint)),
            (Ss, None),
            (Kept, Some(Sound::Kept)),
            (Tl, Some(Sound::NewTimeline)),
            (Warn, Some(Sound::DeviceAhead)),
            (Fail, Some(Sound::CaptureFailed)),
        ];
        for (kind, sound_for) in expected {
            assert_eq!(sound(kind, Sh), sound_for, "{kind:?}");
            assert_eq!(sound(kind, Sw), sound_for, "{kind:?}");
            assert_eq!(sound(kind, D), None, "{kind:?}");
            assert_eq!(sound(kind, Sk), None, "{kind:?}");
        }
    }

    /// Quick keep's feedback: `present` turns admit's outcome into `Presented`, then the hotkey
    /// plays what it still owes.
    fn quick_keep_feedback(popup: &mut Popup, sound_on: bool) -> (Presented, Option<Sound>) {
        let presented = if popup.is_ready() {
            Presented::from(admit(&Games::default(), popup, Kept, None, false).0)
        } else {
            Presented::Unavailable
        };
        (presented, hotkey_sound(Kept, presented, sound_on))
    }

    #[test]
    fn a_kept_result_dropped_under_a_warning_or_error_plays_nothing() {
        for urgent in [Warn, Fail] {
            let mut popup = ready();
            assert_eq!(popup.offer(urgent, false).0, Sh);
            assert_eq!(
                quick_keep_feedback(&mut popup, true),
                (Presented::Dropped, None)
            );
        }
        // Over a checkpoint it swaps in and the popup plays the chime, so the hotkey doesn't.
        let mut popup = ready();
        popup.offer(Cp, false);
        assert_eq!(
            quick_keep_feedback(&mut popup, true),
            (Presented::Shown, None)
        );
    }

    #[test]
    fn with_no_popup_possible_the_kept_sound_plays_once_if_the_sound_is_on() {
        let mut popup = Popup::default();
        assert_eq!(
            quick_keep_feedback(&mut popup, true),
            (Presented::Unavailable, Some(Sound::Kept))
        );
        assert_eq!(
            quick_keep_feedback(&mut popup, false),
            (Presented::Unavailable, None)
        );
        assert_eq!(
            hotkey_sound(Fail, Presented::Unavailable, true),
            Some(Sound::CaptureFailed)
        );
        assert_eq!(hotkey_sound(Fail, Presented::Shown, true), None);
    }

    #[test]
    fn a_popup_records_what_it_shows_so_an_error_holds_its_place() {
        let mut popup = ready();
        assert_eq!(popup.offer(Fail, false).0, Sh);
        let (outcome, id) = popup.offer(Cp, false);
        assert_eq!(outcome, D);
        // A dropped event doesn't take over the error's timer.
        assert_eq!(popup.watch(id, false, false, false), Watch::Keep);
        assert_eq!(popup.offer(Warn, false).0, Sw);
        // Quiet mode's skip leaves the warning alone too.
        let (outcome, id) = popup.offer(Ss, true);
        assert_eq!(outcome, Sk);
        assert_eq!(
            popup.watch(id, false, true, false),
            Watch::Leave { fast: false }
        );
    }

    fn ready() -> Popup {
        let mut popup = Popup::default();
        popup.set_ready();
        popup
    }

    #[test]
    fn nothing_shows_until_the_webview_is_ready() {
        let mut popup = Popup::default();
        assert_eq!(popup.offer(CP, false).0, Outcome::Drop);
        popup.set_ready();
        assert_eq!(popup.offer(CP, false).0, Outcome::Show);
    }

    /// The watcher's look with the game in front and in place, before its time is up.
    const STILL: (bool, bool, bool) = (false, false, false);
    const AWAY: (bool, bool, bool) = (true, false, false);
    const DUE: (bool, bool, bool) = (false, true, false);
    const LATE: (bool, bool, bool) = (false, false, true);

    fn look(popup: &mut Popup, id: u64, (away, due, late): (bool, bool, bool)) -> Watch {
        popup.watch(id, away, due, late)
    }

    #[test]
    fn a_swap_takes_over_the_timer_and_the_exit() {
        let mut popup = ready();
        let (_, first) = popup.offer(CP, false);
        let (outcome, second) = popup.offer(CP, false);
        assert_eq!(outcome, Outcome::Swap);
        // The first popup's watcher neither starts the exit nor hides the window.
        assert_eq!(look(&mut popup, first, DUE), Watch::Stop);
        assert!(!popup.finish(first));
        assert_eq!(look(&mut popup, second, STILL), Watch::Keep);
        assert_eq!(look(&mut popup, second, DUE), Watch::Leave { fast: false });
        assert!(popup.finish(second));
    }

    #[test]
    fn the_game_leaving_the_front_ends_the_popup_with_the_quick_fade() {
        let mut popup = ready();
        let (_, id) = popup.offer(CP, false);
        assert_eq!(look(&mut popup, id, AWAY), Watch::Leave { fast: true });
        assert_eq!(look(&mut popup, id, AWAY), Watch::Keep, "asked once");
    }

    #[test]
    fn the_game_leaving_during_the_fold_switches_it_to_the_quick_fade() {
        let mut popup = ready();
        let (_, id) = popup.offer(CP, false);
        assert_eq!(look(&mut popup, id, DUE), Watch::Leave { fast: false });
        // Still watched while the fold plays.
        assert_eq!(look(&mut popup, id, DUE), Watch::Keep);
        assert_eq!(look(&mut popup, id, AWAY), Watch::Leave { fast: true });
        assert_eq!(look(&mut popup, id, AWAY), Watch::Keep, "asked once");
        assert!(popup.finish(id));
        assert_eq!(look(&mut popup, id, AWAY), Watch::Stop);
    }

    #[test]
    fn an_exit_that_never_ends_hides_the_window_once() {
        let mut popup = ready();
        let (_, id) = popup.offer(CP, false);
        assert_eq!(look(&mut popup, id, DUE), Watch::Leave { fast: false });
        assert_eq!(look(&mut popup, id, LATE), Watch::Hide);
        // A late "done" after that does nothing.
        assert!(!popup.finish(id));
        assert_eq!(look(&mut popup, id, LATE), Watch::Stop);
    }

    #[test]
    fn a_popup_during_the_exit_rises_again_and_the_old_exit_doesnt_touch_it() {
        let mut popup = ready();
        let (_, first) = popup.offer(CP, false);
        assert_eq!(look(&mut popup, first, DUE), Watch::Leave { fast: false });
        let (outcome, second) = popup.offer(CP, false);
        assert_eq!(outcome, Outcome::Show);
        // The first exit's watcher, late "done" and fallback must not end the new popup.
        assert_eq!(look(&mut popup, first, AWAY), Watch::Stop);
        assert!(!popup.finish(first));
        assert_eq!(look(&mut popup, first, LATE), Watch::Stop);
        assert_eq!(look(&mut popup, second, STILL), Watch::Keep);
    }

    #[test]
    fn the_exit_ends_once() {
        let mut popup = ready();
        let (_, id) = popup.offer(CP, false);
        assert!(!popup.finish(id), "not leaving yet");
        assert_eq!(look(&mut popup, id, DUE), Watch::Leave { fast: false });
        assert!(popup.finish(id));
        // The watcher stops; nothing hides twice.
        assert!(!popup.finish(id));
        assert_eq!(look(&mut popup, id, LATE), Watch::Stop);
    }

    #[test]
    fn destroying_or_recreating_the_window_makes_everything_in_flight_stale() {
        let mut popup = ready();
        let (_, showing) = popup.offer(CP, false);
        popup.reset();
        assert_eq!(look(&mut popup, showing, DUE), Watch::Stop);
        assert!(!popup.finish(showing));
        // The new window's webview must say it's ready before anything shows.
        assert_eq!(popup.offer(CP, false).0, Outcome::Drop);
    }

    #[test]
    fn a_reset_during_the_exit_leaves_nothing_to_hide() {
        let mut popup = ready();
        let (_, id) = popup.offer(CP, false);
        assert_eq!(look(&mut popup, id, DUE), Watch::Leave { fast: false });
        popup.reset();
        assert!(!popup.finish(id));
        assert_eq!(look(&mut popup, id, LATE), Watch::Stop);
    }

    #[test]
    fn only_games_with_an_install_dir_keep_the_window_and_the_last_one_ends_it() {
        let mut games = Games::default();
        assert!(!games.started(1, None));
        // A game that never counted can't end the window.
        assert!(!games.stopped(1));
        assert!(games.started(2, Some(DIR)));
        assert!(games.started(3, Some(r"D:\Other")));
        assert!(!games.stopped(2));
        assert!(games.stopped(3));
        // A repeated stop does nothing.
        assert!(!games.stopped(3));
    }

    const DIR: &str = r"C:\Games\Lantern Knight";

    #[test]
    fn the_front_window_belongs_to_the_game_it_runs_from() {
        let mut games = Games::default();
        games.started(7, Some(DIR));
        games.started(8, Some(r"C:\Games\Other"));
        // The launcher, splash and main window all count: any exe under the install dir.
        assert_eq!(
            games.running_from(r"C:\Games\Lantern Knight\bin\knight.exe"),
            Some(7)
        );
        assert_eq!(games.running_from(r"c:\games\other\launcher.exe"), Some(8));
        // A sibling folder with the same prefix, or anything else, isn't the game.
        assert_eq!(games.running_from(r"C:\Games\Lantern Knight 2\k.exe"), None);
        assert_eq!(games.running_from(r"C:\Windows\explorer.exe"), None);
        games.stopped(7);
        assert_eq!(
            games.running_from(r"C:\Games\Lantern Knight\bin\knight.exe"),
            None
        );
    }

    #[test]
    fn a_session_starts_once_per_game_started() {
        let mut games = Games::default();
        games.started(7, Some(DIR));
        let run = games.take_session(7).expect("armed by GameStarted");
        assert_eq!(games.take_session(7), None, "only the first time in front");
        assert_eq!(games.install_dir(7, run), Some(DIR));
        // An untracked or unknown game has nothing to take.
        assert_eq!(games.take_session(9), None);
    }

    #[test]
    fn game_stopped_disarms_and_a_new_run_makes_the_old_one_stale() {
        let mut games = Games::default();
        games.started(7, Some(DIR));
        games.stopped(7);
        assert_eq!(games.take_session(7), None, "disarmed");

        games.started(7, Some(DIR));
        let first = games.take_session(7).unwrap();
        // The game quits and starts again while the first run's delayed check waits.
        games.stopped(7);
        assert_eq!(games.install_dir(7, first), None);
        games.started(7, Some(DIR));
        assert_eq!(games.install_dir(7, first), None, "that run is over");
        let second = games.take_session(7).expect("re-armed");
        assert_ne!(first, second);
        assert_eq!(games.install_dir(7, second), Some(DIR));
    }

    /// The delayed session start of run 1 passed its early check; then the game stopped and
    /// started again before it reached the popup. Only run 2's may show.
    #[test]
    fn a_session_start_from_a_run_that_ended_after_its_check_never_shows() {
        let mut games = Games::default();
        let mut popup = ready();
        games.started(7, Some(DIR));
        let first = games.take_session(7).unwrap();
        assert!(
            games.install_dir(7, first).is_some(),
            "the early check passes"
        );
        games.stopped(7);
        games.started(7, Some(DIR));
        let second = games.take_session(7).unwrap();

        let (outcome, id) = admit(&games, &mut popup, Ss, Some((7, first)), false);
        assert_eq!((outcome, sound(Ss, outcome)), (D, None));
        assert_eq!(id, 0, "nothing took the popup");
        assert_eq!(
            admit(&games, &mut popup, Ss, Some((7, second)), false).0,
            Sh
        );
        // Stopped for good: its own delayed work is stale too.
        games.stopped(7);
        assert_eq!(admit(&games, &mut popup, Ss, Some((7, second)), false).0, D);
        // Events that aren't a session start don't name a run.
        assert_eq!(admit(&games, &mut popup, Cp, None, false).0, Sw);
    }

    // ---- device ahead ----

    fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    fn status(relation: Divergence, auto_applied: bool) -> GameSyncStatus {
        GameSyncStatus {
            game_uuid: id(1),
            relation,
            remote_device: Some("Steam Deck".into()),
            remote_version_uuid: Some(id(13)),
            remote_version_id: None,
            auto_applied,
        }
    }

    /// The remote's line: 10 ← 11 ← 12 ← 13 (another device's tip), and 11 ← 20.
    fn line() -> Parents {
        [
            (id(10), None),
            (id(11), Some(id(10))),
            (id(12), Some(id(11))),
            (id(13), Some(id(12))),
            (id(20), Some(id(11))),
        ]
        .into_iter()
        .collect()
    }

    fn ahead(warn: bool, cloud: bool, status: Option<&GameSyncStatus>) -> Option<Ahead> {
        device_ahead(warn, cloud, status, &line(), Some(id(10)))
    }

    #[test]
    fn a_fast_forward_not_yet_applied_warns_with_the_device_and_count() {
        let ff = status(Divergence::FastForward, false);
        assert_eq!(
            ahead(true, true, Some(&ff)),
            Some(Ahead {
                device: "Steam Deck".into(),
                count: 3
            })
        );
        // Counted on the remote's line: the tip needn't be pulled yet.
        assert_eq!(ff.remote_version_id, None);
        // From where this device is now: after a restore to 11 it's two behind.
        let after_restore = device_ahead(true, true, Some(&ff), &line(), Some(id(11)));
        assert_eq!(after_restore.map(|a| a.count), Some(2));
    }

    #[test]
    fn anything_else_is_the_plain_session_start() {
        let ff = status(Divergence::FastForward, false);
        assert_eq!(
            ahead(true, true, Some(&status(Divergence::FastForward, true))),
            None,
            "auto-applied"
        );
        assert_eq!(
            ahead(true, true, Some(&status(Divergence::InSync, false))),
            None,
            "in sync"
        );
        for relation in [
            Divergence::LocalAhead,
            Divergence::Diverged,
            Divergence::RemoteOnly,
            Divergence::LocalOnly,
        ] {
            assert_eq!(
                ahead(true, true, Some(&status(relation, false))),
                None,
                "{relation:?}"
            );
        }
        assert_eq!(ahead(true, false, Some(&ff)), None, "cloud off");
        assert_eq!(ahead(false, true, Some(&ff)), None, "warn-ahead off");
        assert_eq!(ahead(true, true, None), None, "unknown");
    }

    #[test]
    fn a_warning_it_cant_word_isnt_guessed() {
        let mut ff = status(Divergence::FastForward, false);
        ff.remote_device = None;
        assert_eq!(ahead(true, true, Some(&ff)), None, "no device");
        let mut ff = status(Divergence::FastForward, false);
        ff.remote_version_uuid = None;
        assert_eq!(ahead(true, true, Some(&ff)), None, "no tip");
        let ff = status(Divergence::FastForward, false);
        // This device moved off the tip's line since, is at the tip, or has no version.
        let at = |local| device_ahead(true, true, Some(&ff), &line(), local);
        assert_eq!(at(Some(id(20))), None);
        assert_eq!(at(Some(id(13))), None);
        assert_eq!(at(None), None);
        // The tip isn't on the line read with the status.
        let empty = Parents::new();
        assert_eq!(
            device_ahead(true, true, Some(&ff), &empty, Some(id(10))),
            None
        );
        // A parent cycle in a broken history ends.
        let broken: Parents = [(id(13), Some(id(12))), (id(12), Some(id(13)))]
            .into_iter()
            .collect();
        assert_eq!(
            device_ahead(true, true, Some(&ff), &broken, Some(id(10))),
            None
        );
    }
}

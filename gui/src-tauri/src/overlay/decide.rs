//! The overlay's decisions, free of Win32 so they're tested on every platform: the corner, where
//! the window goes, what a new event does to the popup on screen, and which sound goes with it.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::time::Duration;

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

/// What a popup is about. O2 adds session start, new timeline, device ahead and capture failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Checkpoint,
}

impl Kind {
    /// Warnings and errors, which nothing else replaces (handoff, "Rules").
    fn urgent(self) -> bool {
        match self {
            Kind::Checkpoint => false,
        }
    }

    /// How long the popup stays before it folds away. Quiet mode shows only the mark, briefly.
    pub fn duration(self, quiet: bool) -> Duration {
        match self {
            Kind::Checkpoint if quiet => Duration::from_millis(1600),
            Kind::Checkpoint => Duration::from_millis(3200),
        }
    }
}

/// What an incoming event does to the popup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing is showing: the popup rises.
    Show,
    /// A popup is showing: its content changes in place and its timer restarts. Never stacked.
    Swap,
    /// Dropped, not queued.
    Drop,
}

/// `incoming`'s outcome while `showing` is on screen.
pub fn outcome(showing: Option<Kind>, incoming: Kind) -> Outcome {
    match showing {
        None => Outcome::Show,
        Some(current) if current.urgent() && !incoming.urgent() => Outcome::Drop,
        Some(_) => Outcome::Swap,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sound {
    /// Automatic checkpoint (and session start, O2).
    Soft,
}

/// The sound that goes with `outcome` for `kind`: the incoming event's sound on a swap, nothing
/// when it's dropped.
pub fn sound(kind: Kind, outcome: Outcome) -> Option<Sound> {
    match (outcome, kind) {
        (Outcome::Drop, _) => None,
        (Outcome::Show | Outcome::Swap, Kind::Checkpoint) => Some(Sound::Soft),
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
    pub fn offer(&mut self, incoming: Kind) -> (Outcome, u64) {
        let showing = match self.phase {
            Phase::Showing(kind) => Some(kind),
            Phase::Hidden | Phase::Leaving { .. } => None,
        };
        let outcome = if self.ready {
            outcome(showing, incoming)
        } else {
            Outcome::Drop
        };
        if outcome != Outcome::Drop {
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

/// The running games that can get a popup: tracked, with an install dir. The overlay window
/// exists while there is one.
#[derive(Debug, Default)]
pub struct Games(HashSet<i32>);

impl Games {
    /// Records a started game; true if it counts (it has an install dir).
    pub fn started(&mut self, game_id: i32, has_install_dir: bool) -> bool {
        if has_install_dir {
            self.0.insert(game_id);
        }
        has_install_dir
    }

    /// Records a stopped game; true if it was the last one that counted.
    pub fn stopped(&mut self, game_id: i32) -> bool {
        self.0.remove(&game_id) && self.0.is_empty()
    }
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

    #[test]
    fn a_checkpoint_while_one_shows_swaps_instead_of_stacking() {
        assert_eq!(outcome(None, Kind::Checkpoint), Outcome::Show);
        assert_eq!(
            outcome(Some(Kind::Checkpoint), Kind::Checkpoint),
            Outcome::Swap
        );
    }

    #[test]
    fn a_shown_or_swapped_checkpoint_plays_the_soft_sound_and_a_dropped_one_nothing() {
        assert_eq!(sound(Kind::Checkpoint, Outcome::Show), Some(Sound::Soft));
        assert_eq!(sound(Kind::Checkpoint, Outcome::Swap), Some(Sound::Soft));
        assert_eq!(sound(Kind::Checkpoint, Outcome::Drop), None);
    }

    #[test]
    fn quiet_mode_shows_the_checkpoint_for_less_time() {
        assert_eq!(
            Kind::Checkpoint.duration(false),
            Duration::from_millis(3200)
        );
        assert_eq!(Kind::Checkpoint.duration(true), Duration::from_millis(1600));
    }

    fn ready() -> Popup {
        let mut popup = Popup::default();
        popup.set_ready();
        popup
    }

    #[test]
    fn nothing_shows_until_the_webview_is_ready() {
        let mut popup = Popup::default();
        assert_eq!(popup.offer(Kind::Checkpoint).0, Outcome::Drop);
        popup.set_ready();
        assert_eq!(popup.offer(Kind::Checkpoint).0, Outcome::Show);
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
        let (_, first) = popup.offer(Kind::Checkpoint);
        let (outcome, second) = popup.offer(Kind::Checkpoint);
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
        let (_, id) = popup.offer(Kind::Checkpoint);
        assert_eq!(look(&mut popup, id, AWAY), Watch::Leave { fast: true });
        assert_eq!(look(&mut popup, id, AWAY), Watch::Keep, "asked once");
    }

    #[test]
    fn the_game_leaving_during_the_fold_switches_it_to_the_quick_fade() {
        let mut popup = ready();
        let (_, id) = popup.offer(Kind::Checkpoint);
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
        let (_, id) = popup.offer(Kind::Checkpoint);
        assert_eq!(look(&mut popup, id, DUE), Watch::Leave { fast: false });
        assert_eq!(look(&mut popup, id, LATE), Watch::Hide);
        // A late "done" after that does nothing.
        assert!(!popup.finish(id));
        assert_eq!(look(&mut popup, id, LATE), Watch::Stop);
    }

    #[test]
    fn a_popup_during_the_exit_rises_again_and_the_old_exit_doesnt_touch_it() {
        let mut popup = ready();
        let (_, first) = popup.offer(Kind::Checkpoint);
        assert_eq!(look(&mut popup, first, DUE), Watch::Leave { fast: false });
        let (outcome, second) = popup.offer(Kind::Checkpoint);
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
        let (_, id) = popup.offer(Kind::Checkpoint);
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
        let (_, showing) = popup.offer(Kind::Checkpoint);
        popup.reset();
        assert_eq!(look(&mut popup, showing, DUE), Watch::Stop);
        assert!(!popup.finish(showing));
        // The new window's webview must say it's ready before anything shows.
        assert_eq!(popup.offer(Kind::Checkpoint).0, Outcome::Drop);
    }

    #[test]
    fn a_reset_during_the_exit_leaves_nothing_to_hide() {
        let mut popup = ready();
        let (_, id) = popup.offer(Kind::Checkpoint);
        assert_eq!(look(&mut popup, id, DUE), Watch::Leave { fast: false });
        popup.reset();
        assert!(!popup.finish(id));
        assert_eq!(look(&mut popup, id, LATE), Watch::Stop);
    }

    #[test]
    fn only_games_with_an_install_dir_keep_the_window_and_the_last_one_ends_it() {
        let mut games = Games::default();
        assert!(!games.started(1, false));
        // A game that never counted can't end the window.
        assert!(!games.stopped(1));
        assert!(games.started(2, true));
        assert!(games.started(3, true));
        assert!(!games.stopped(2));
        assert!(games.stopped(3));
        // A repeated stop does nothing.
        assert!(!games.stopped(3));
    }
}

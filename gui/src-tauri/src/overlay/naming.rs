//! Name it (O4, handoff "Naming card"): the press keeps the checkpoint first, then a card asks for
//! a name, over the game when it can show there, otherwise in Worldline. The decisions here are
//! pure; `mod.rs` does the windows and the renaming.

use serde::{Deserialize, Serialize};

/// The longest name the card takes, in characters.
pub const MAX_NAME: usize = 60;

/// Where the naming card opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// Over the game, in the overlay window, which takes focus for it.
    Overlay,
    /// In Worldline's main window, brought to the front.
    App,
}

/// Over the game only when the popup is on for it (`popup_on`), the game is still the window in
/// front with the overlay ready (`gate`), Windows doesn't report a Direct3D exclusive
/// fullscreen app (`fullscreen`), and the overlay then takes the focus (`focus`, tried last and
/// only when everything else allows it). Otherwise in the app.
pub fn place(popup_on: bool, gate: bool, fullscreen: bool, focus: impl FnOnce() -> bool) -> Place {
    if popup_on && gate && !fullscreen && focus() {
        Place::Overlay
    } else {
        Place::App
    }
}

/// How the card was closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum How {
    /// Enter or ✓: the typed name, or the automatic one when it's empty.
    Save,
    /// Esc: the automatic name.
    Escape,
    /// The card lost the focus: what was typed is saved, and the focus stays where it went.
    Blur,
}

impl How {
    /// Only an explicit close hands the focus back to the game.
    pub fn hands_back(self) -> bool {
        matches!(self, How::Save | How::Escape)
    }
}

/// The name the checkpoint ends up with: Esc keeps `auto`; otherwise the typed name, trimmed and
/// cut to `MAX_NAME` characters, or `auto` when nothing is left.
pub fn final_name(how: How, typed: &str, auto: &str) -> String {
    let typed: String = typed.trim().chars().take(MAX_NAME).collect();
    let typed = typed.trim_end();
    if how == How::Escape || typed.is_empty() {
        auto.to_owned()
    } else {
        typed.to_owned()
    }
}

/// The rename to make, if any: only when `name` isn't the tag the version has.
pub fn rename<'a>(tag: Option<&str>, name: &'a str) -> Option<&'a str> {
    (tag != Some(name)).then_some(name)
}

/// After an app-mode close: whether to hand the focus back to the game, given whether its window
/// is still there, and whether to hide Worldline to the tray again: only once the name was saved
/// (`renamed`), so a failure stays in sight.
pub fn app_return(how: How, game_window: bool, was_tray: bool, renamed: bool) -> (bool, bool) {
    let hand_back = how.hands_back() && game_window;
    (hand_back, hand_back && was_tray && renamed)
}

/// What closing a card in the app did: whether the focus went back to the game, and why the
/// checkpoint couldn't be renamed, if it couldn't.
#[derive(Debug, Default, Serialize)]
pub struct NameItClosed {
    pub handed: bool,
    pub error: Option<String>,
}

/// An open card: the version the press kept, and what to go back to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub id: u64,
    pub game_id: i32,
    pub version_id: i32,
    pub version_number: i32,
    /// The tag the kept version has: the automatic name, or the one it already had.
    pub auto: String,
    /// The game's window at the press.
    pub game_window: isize,
    pub place: Place,
    /// Worldline was hidden in the tray before an app-mode card brought it out.
    pub was_tray: bool,
    phase: Phase,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Phase {
    /// Taking a name. Over the game, the overlay window once it had the focus.
    Open { focused: Option<isize> },
    /// Over the game: named `name` once renamed, the card folds and flies to the popup's corner,
    /// `landed` once the webview says it got there.
    Closing {
        how: How,
        name: Option<String>,
        landed: bool,
    },
}

/// What a card is opened for.
#[derive(Debug, Clone)]
pub struct Opening {
    pub game_id: i32,
    pub version_id: i32,
    pub version_number: i32,
    pub auto: String,
    pub game_window: isize,
    pub place: Place,
    pub was_tray: bool,
}

/// At most one card at a time, from the press that keeps for it until it's gone. Every card gets
/// a new id, and only its own close counts, once.
#[derive(Debug, Default)]
pub struct Naming {
    last: u64,
    /// A Name it press is keeping, before its card opens.
    pending: bool,
    open: Option<Session>,
}

impl Naming {
    /// Claims the card for a Name it press; false, and the press is ignored, while another press
    /// keeps for one or a card is open.
    pub fn begin(&mut self) -> bool {
        if self.pending || self.open.is_some() {
            return false;
        }
        self.pending = true;
        true
    }

    /// The press that `begin`s ended without a card.
    pub fn end(&mut self) {
        self.pending = false;
    }

    /// A card over the game, open or folding: the popup can't show meanwhile.
    pub fn over_game(&self) -> bool {
        self.open
            .as_ref()
            .is_some_and(|s| s.place == Place::Overlay)
    }

    /// Opens a card; `None` when one is open already.
    pub fn open(&mut self, opening: Opening) -> Option<&Session> {
        if self.open.is_some() {
            return None;
        }
        self.pending = false;
        self.last += 1;
        self.open = Some(Session {
            id: self.last,
            game_id: opening.game_id,
            version_id: opening.version_id,
            version_number: opening.version_number,
            auto: opening.auto,
            game_window: opening.game_window,
            place: opening.place,
            was_tray: opening.was_tray,
            phase: Phase::Open { focused: None },
        });
        self.open.as_ref()
    }

    /// Drops card `id` before anyone saw it: the overlay couldn't take the focus.
    pub fn cancel(&mut self, id: u64) -> Option<Session> {
        self.open.take_if(|s| s.id == id)
    }

    /// The overlay window `window` has the focus for card `id`.
    pub fn focused(&mut self, id: u64, window: isize) {
        if let Some(Session {
            phase: Phase::Open { focused },
            ..
        }) = self.open.as_mut().filter(|s| s.id == id)
        {
            *focused = Some(window);
        }
    }

    /// The card over the game that `foreground` just took the focus from: one that had it, and
    /// is still taking a name.
    pub fn focus_lost(&self, foreground: isize) -> Option<u64> {
        let session = self.open.as_ref()?;
        match session.phase {
            Phase::Open {
                focused: Some(window),
            } if session.place == Place::Overlay && window != foreground => Some(session.id),
            _ => None,
        }
    }

    /// Card `id` is still taking a name.
    pub fn is_open(&self, id: u64) -> bool {
        self.open
            .as_ref()
            .is_some_and(|s| s.id == id && matches!(s.phase, Phase::Open { .. }))
    }

    /// Closes card `id` in `place` the first time it's asked: its session to rename and return
    /// from. A card in the app is gone then; one over the game folds until it's `finish`ed.
    pub fn close(&mut self, id: u64, place: Place, how: How) -> Option<Session> {
        let session = self
            .open
            .as_mut()
            .filter(|s| s.id == id && s.place == place)?;
        if !matches!(session.phase, Phase::Open { .. }) {
            return None;
        }
        match place {
            Place::App => self.open.take(),
            Place::Overlay => {
                session.phase = Phase::Closing {
                    how,
                    name: None,
                    landed: false,
                };
                Some(session.clone())
            }
        }
    }

    /// Card `id`, folding over the game, ended up named `renamed`.
    pub fn named(&mut self, id: u64, renamed: String) {
        if let Some(Session {
            phase: Phase::Closing { name, .. },
            ..
        }) = self.open.as_mut().filter(|s| s.id == id)
        {
            *name = Some(renamed);
        }
    }

    /// The webview says card `id` has flown to its corner.
    pub fn land(&mut self, id: u64) {
        if let Some(Session {
            phase: Phase::Closing { landed, .. },
            ..
        }) = self.open.as_mut().filter(|s| s.id == id)
        {
            *landed = true;
        }
    }

    /// Card `id` is still flying to its corner: closed, and neither landed nor gone.
    pub fn folding(&self, id: u64) -> bool {
        self.open
            .as_ref()
            .is_some_and(|s| s.id == id && matches!(s.phase, Phase::Closing { landed: false, .. }))
    }

    /// Game `game_id`'s card over it, while it's still taking a name.
    pub fn taking_name(&self, game_id: i32) -> Option<u64> {
        self.open
            .as_ref()
            .filter(|s| {
                s.place == Place::Overlay
                    && s.game_id == game_id
                    && matches!(s.phase, Phase::Open { .. })
            })
            .map(|s| s.id)
    }

    /// Card `id` is past its rename: renamed, or gone.
    pub fn settled(&self, id: u64) -> bool {
        self.open
            .as_ref()
            .filter(|s| s.id == id)
            .is_none_or(|s| matches!(s.phase, Phase::Closing { name: Some(_), .. }))
    }

    /// Ends card `id` over the game, landed or not: its session, how it was closed and the name
    /// it ended up with, once. Nothing while it's open, or before it's renamed.
    pub fn finish(&mut self, id: u64) -> Option<(Session, How, String)> {
        let (how, name) = match &self.open.as_ref().filter(|s| s.id == id)?.phase {
            Phase::Closing {
                how,
                name: Some(name),
                ..
            } => (*how, name.clone()),
            _ => return None,
        };
        Some((self.open.take()?, how, name))
    }

    /// The overlay window is going away while game `game_id`'s card is over it: dropped, with
    /// the name the press kept or the one it was renamed to.
    pub fn abandon(&mut self, game_id: i32) -> Option<Session> {
        self.open
            .take_if(|s| s.place == Place::Overlay && s.game_id == game_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn the_card_opens_over_the_game_only_when_every_condition_holds() {
        assert_eq!(place(true, true, false, || true), Place::Overlay);
        for (popup_on, gate, fullscreen) in [
            (false, true, false),
            (true, false, false),
            (true, true, true),
        ] {
            let tried = Cell::new(false);
            let focus = || {
                tried.set(true);
                true
            };
            assert_eq!(place(popup_on, gate, fullscreen, focus), Place::App);
            // The focus isn't taken for a card that can't show over the game anyway.
            assert!(!tried.get(), "{popup_on} {gate} {fullscreen}");
        }
        // Windows refused the overlay the focus.
        assert_eq!(place(true, true, false, || false), Place::App);
    }

    #[test]
    fn enter_and_check_save_the_trimmed_name_or_the_automatic_one_when_empty() {
        let auto = "Kept in game, 21:04";
        assert_eq!(
            final_name(How::Save, "  Before the boss ", auto),
            "Before the boss"
        );
        assert_eq!(final_name(How::Save, "   ", auto), auto);
        assert_eq!(final_name(How::Save, "", auto), auto);
    }

    #[test]
    fn escape_keeps_the_automatic_name_whatever_was_typed() {
        assert_eq!(final_name(How::Escape, "Before the boss", "Auto"), "Auto");
    }

    #[test]
    fn losing_the_focus_saves_what_was_typed() {
        assert_eq!(final_name(How::Blur, "Half typ", "Auto"), "Half typ");
        assert_eq!(final_name(How::Blur, "", "Auto"), "Auto");
    }

    #[test]
    fn a_name_is_cut_to_sixty_characters() {
        let long = "ñ".repeat(70);
        assert_eq!(
            final_name(How::Save, &long, "Auto").chars().count(),
            MAX_NAME
        );
        // Cutting doesn't leave a trailing space.
        let spaced = format!("{} b", "a".repeat(59));
        assert_eq!(final_name(How::Save, &spaced, "Auto"), "a".repeat(59));
    }

    #[test]
    fn only_a_changed_name_is_renamed() {
        assert_eq!(rename(Some("Auto"), "Auto"), None);
        assert_eq!(rename(Some("Auto"), "Boss"), Some("Boss"));
        assert_eq!(rename(None, "Boss"), Some("Boss"));
    }

    #[test]
    fn only_an_explicit_close_hands_the_focus_back() {
        assert!(How::Save.hands_back());
        assert!(How::Escape.hands_back());
        assert!(!How::Blur.hands_back());
    }

    #[test]
    fn the_app_hands_back_to_a_game_window_still_there_and_returns_to_the_tray_it_came_from() {
        // Explicit, the game's window there: back to the game, and Worldline back to the tray.
        assert_eq!(app_return(How::Save, true, true, true), (true, true));
        assert_eq!(app_return(How::Escape, true, false, true), (true, false));
        // The game's window is gone: Worldline stays, out of the tray.
        assert_eq!(app_return(How::Save, false, true, true), (false, false));
        // The focus went elsewhere: it stays there, and Worldline stays out.
        assert_eq!(app_return(How::Blur, true, true, true), (false, false));
        // The name couldn't be saved: back to the game, but Worldline stays out to say so.
        assert_eq!(app_return(How::Save, true, true, false), (true, false));
    }

    fn opening(place: Place) -> Opening {
        Opening {
            game_id: 7,
            version_id: 40,
            version_number: 12,
            auto: "Auto".into(),
            game_window: 0x1234,
            place,
            was_tray: false,
        }
    }

    const OVERLAY: isize = 0x99;

    #[test]
    fn a_press_is_ignored_while_another_keeps_or_its_card_is_open() {
        let mut naming = Naming::default();
        assert!(naming.begin());
        // A second press while the first one keeps.
        assert!(!naming.begin());
        let id = naming.open(opening(Place::App)).unwrap().id;
        assert!(!naming.begin());
        naming.close(id, Place::App, How::Save).unwrap();
        assert!(naming.begin());
        // A press whose keep failed frees the card.
        naming.end();
        assert!(naming.begin());
    }

    #[test]
    fn every_card_gets_a_new_id() {
        let mut naming = Naming::default();
        let first = naming.open(opening(Place::App)).unwrap().id;
        assert!(naming.open(opening(Place::App)).is_none());
        naming.close(first, Place::App, How::Save).unwrap();
        let next = naming.open(opening(Place::App)).unwrap().id;
        assert_ne!(next, first);
    }

    #[test]
    fn a_card_closes_once_only_for_its_own_id_and_place() {
        let mut naming = Naming::default();
        let id = naming.open(opening(Place::App)).unwrap().id;
        assert!(naming.close(id + 1, Place::App, How::Save).is_none());
        // The overlay's event can't close a card in the app.
        assert!(naming.close(id, Place::Overlay, How::Save).is_none());
        assert!(naming.is_open(id));
        let closed = naming.close(id, Place::App, How::Blur).unwrap();
        assert_eq!(closed.version_id, 40);
        // The blur after an explicit close, or a second ✓, finds nothing.
        assert!(naming.close(id, Place::App, How::Save).is_none());
        assert!(!naming.is_open(id));
    }

    #[test]
    fn a_card_over_the_game_folds_until_it_has_landed_and_was_renamed() {
        let mut naming = Naming::default();
        let id = naming.open(opening(Place::Overlay)).unwrap().id;
        assert!(naming.over_game());
        naming.land(id);
        assert!(naming.finish(id).is_none(), "it hasn't closed yet");
        naming.close(id, Place::Overlay, How::Escape).unwrap();
        // Still over the game while it folds and flies: no popup, no second card.
        assert!(naming.over_game() && !naming.begin());
        assert!(naming.close(id, Place::Overlay, How::Blur).is_none());
        assert!(naming.folding(id));
        naming.land(id);
        assert!(!naming.folding(id));
        assert!(
            naming.finish(id).is_none(),
            "landed before the rename finished"
        );
        naming.named(id, "Auto".into());
        let (session, how, name) = naming.finish(id).unwrap();
        assert_eq!((session.id, how, name.as_str()), (id, How::Escape, "Auto"));
        assert!(!naming.over_game());
        assert!(naming.finish(id).is_none());
    }

    #[test]
    fn a_renamed_card_that_never_lands_still_finishes() {
        let mut naming = Naming::default();
        let id = naming.open(opening(Place::Overlay)).unwrap().id;
        naming.close(id, Place::Overlay, How::Save).unwrap();
        naming.named(id, "Boss".into());
        assert!(naming.folding(id));
        assert_eq!(naming.finish(id).unwrap().2, "Boss");
        // Gone, so nothing waits for it to land any more.
        assert!(!naming.folding(id));
    }

    #[test]
    fn a_stopping_game_waits_for_its_card_to_save_what_was_typed() {
        let mut naming = Naming::default();
        let id = naming.open(opening(Place::Overlay)).unwrap().id;
        assert_eq!(naming.taking_name(8), None);
        assert_eq!(naming.taking_name(7), Some(id));
        assert!(!naming.settled(id));
        // The webview sent what was typed: still renaming.
        naming.close(id, Place::Overlay, How::Blur).unwrap();
        assert_eq!(naming.taking_name(7), None);
        assert!(!naming.settled(id));
        naming.named(id, "Half typ".into());
        assert!(naming.settled(id));
        naming.abandon(7).unwrap();
        // Gone counts as settled; a card in the app isn't the game's to wait for.
        assert!(naming.settled(id));
        naming.open(opening(Place::App)).unwrap();
        assert_eq!(naming.taking_name(7), None);
    }

    #[test]
    fn the_focus_is_lost_only_after_the_overlay_had_it_and_while_the_card_takes_a_name() {
        let mut naming = Naming::default();
        let id = naming.open(opening(Place::Overlay)).unwrap().id;
        // A foreground change from before the overlay took the focus.
        assert_eq!(naming.focus_lost(0x1234), None);
        naming.focused(id + 1, OVERLAY);
        assert_eq!(naming.focus_lost(0x1234), None);
        naming.focused(id, OVERLAY);
        assert_eq!(naming.focus_lost(OVERLAY), None);
        assert_eq!(naming.focus_lost(0x1234), Some(id));
        // Folding: handing the focus back isn't losing it.
        naming.close(id, Place::Overlay, How::Save);
        assert_eq!(naming.focus_lost(0x1234), None);
    }

    #[test]
    fn an_app_card_never_loses_the_overlays_focus() {
        let mut naming = Naming::default();
        let id = naming.open(opening(Place::App)).unwrap().id;
        naming.focused(id, OVERLAY);
        assert_eq!(naming.focus_lost(0x1234), None);
        assert!(!naming.over_game());
    }

    #[test]
    fn a_card_the_overlay_couldnt_take_the_focus_for_is_dropped_unseen() {
        let mut naming = Naming::default();
        let id = naming.open(opening(Place::Overlay)).unwrap().id;
        assert!(naming.cancel(id + 1).is_none());
        assert_eq!(naming.cancel(id).unwrap().id, id);
        // Then the app's card opens for the same press.
        assert!(naming.open(opening(Place::App)).is_some());
    }

    #[test]
    fn a_stopped_game_drops_its_card_over_the_game_but_not_an_app_one() {
        let mut naming = Naming::default();
        let id = naming.open(opening(Place::Overlay)).unwrap().id;
        assert!(naming.abandon(8).is_none());
        assert_eq!(naming.abandon(7).unwrap().id, id);
        assert!(!naming.over_game());
        // The late close and landing of the dropped card do nothing.
        assert!(naming.close(id, Place::Overlay, How::Save).is_none());
        naming.land(id);
        assert!(naming.finish(id).is_none());

        let id = naming.open(opening(Place::App)).unwrap().id;
        assert!(naming.abandon(7).is_none());
        assert!(naming.is_open(id));
    }
}

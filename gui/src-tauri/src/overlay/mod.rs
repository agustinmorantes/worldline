//! The game overlay (ADR 0009): a small popup drawn over the game when something happens to its
//! saves, in a window of its own. Everything is decided here: the gate, the popup state, timers,
//! placement and sound. The overlay webview (`gui/overlay.html`) only renders what it's sent,
//! and says when its exit animation has ended.
//!
//! Windows only; elsewhere every entry point is a no-op. The hotkeys (`hotkeys.rs`) follow the
//! game in front and share the popup. Name it's card (`naming.rs`) takes the whole window, and the
//! focus, while it's open over the game.
//!
//! Never take `hotkeys`, `front`, `games`, `popup` or `naming` on the main thread: window calls
//! from other threads wait for the main thread, so a lock held there would deadlock them. Take
//! them in that order.

#[cfg_attr(not(windows), allow(dead_code))]
mod decide;
#[cfg(windows)]
mod hotkeys;
#[cfg(windows)]
mod keep;
#[cfg_attr(not(windows), allow(dead_code))]
mod naming;
#[cfg(windows)]
mod sound;
#[cfg(windows)]
mod win;

use decide::{Corner, GameCorner};
pub use naming::{How, NameItClosed};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};
use worldline_core::detection::{BranchFrom, CaptureFailure};

/// The overlay preferences the GUI pushes (`set_overlay_config`), with the motion and language
/// it resolved. The GUI owns the schema (`gui/src/shared/prefs/schema.ts`).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(not(windows), allow(dead_code))]
pub struct OverlayConfig {
    enabled: bool,
    sound: bool,
    corner: Corner,
    /// By game `uuid`.
    per_game: HashMap<String, GameCorner>,
    quiet: bool,
    warn_ahead: bool,
    /// The hotkeys' accelerators (`hotkeys::parse`); `""` is off.
    quick_keep_key: String,
    name_it_key: String,
    /// The hotkeys' automatic tag in the app language, with `{time}` to fill in.
    kept_tag_template: String,
    motion: Motion,
    language: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Motion {
    Full,
    Reduced,
}

/// The overlay's state, managed by Tauri.
#[derive(Default)]
pub struct Overlay {
    /// `None` until the GUI pushes it; no popup shows before that.
    config: Mutex<Option<OverlayConfig>>,
    /// The running game whose window is in front, if any, and its run.
    #[cfg_attr(not(windows), allow(dead_code))]
    front: Mutex<Option<decide::Front>>,
    /// Also the lifecycle lock: the window and the foreground hook are only created or destroyed
    /// while it's held.
    #[cfg_attr(not(windows), allow(dead_code))]
    games: Mutex<decide::Games>,
    #[cfg_attr(not(windows), allow(dead_code))]
    popup: Mutex<decide::Popup>,
    #[cfg_attr(not(windows), allow(dead_code))]
    naming: Mutex<naming::Naming>,
    #[cfg(windows)]
    hook: Mutex<Option<win::ForegroundHook>>,
    /// The hotkeys registered now.
    #[cfg(windows)]
    hotkeys: Mutex<Vec<hotkeys::Entry>>,
    #[cfg(windows)]
    cooldown: Mutex<hotkeys::Cooldown>,
    /// Moves on whenever the front, a run or the hotkeys change, so a late press is stale.
    #[cfg(windows)]
    generation: hotkeys::Generation,
}

impl Overlay {
    pub fn set_config(&self, config: OverlayConfig) {
        *lock(&self.config) = Some(config);
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(windows)]
use windows::pressed;
#[cfg(windows)]
pub use windows::{
    check_shortcut, checkpoint_failed, config_changed, game_started, game_stopped, name_it_closed,
    setup, version_created,
};

#[cfg(not(windows))]
pub fn setup(app: &tauri::App) {
    use tauri::Manager;
    app.manage(Overlay::default());
}

#[cfg(not(windows))]
pub fn game_started(_app: &tauri::AppHandle, _game_id: i32) {}

#[cfg(not(windows))]
pub fn game_stopped(_app: &tauri::AppHandle, _game_id: i32) {}

#[cfg(not(windows))]
pub fn version_created(
    _app: &tauri::AppHandle,
    _game_id: i32,
    _version_number: i32,
    _branch_from: Option<BranchFrom>,
) {
}

#[cfg(not(windows))]
pub fn checkpoint_failed(_app: &tauri::AppHandle, _game_id: i32, _reason: CaptureFailure) {}

#[cfg(not(windows))]
pub fn config_changed(_app: &tauri::AppHandle) {}

/// No hotkeys here, so no card is ever open.
#[cfg(not(windows))]
pub fn name_it_closed(
    _app: &tauri::AppHandle,
    _session: u64,
    _how: How,
    _name: &str,
) -> NameItClosed {
    NameItClosed::default()
}

/// No hotkeys here, so none is taken.
#[cfg(not(windows))]
pub fn check_shortcut(_app: &tauri::AppHandle, _accelerator: &str) -> Result<(), String> {
    Ok(())
}

#[cfg(windows)]
mod windows {
    use super::decide::{self, Kind, Outcome, Presented, Rect, Sound, Watch};
    use super::hotkeys::{self, Action, Press};
    use super::keep::NotKept;
    use super::naming::{self, How, NameItClosed, Opening, Place, Session};
    use super::{
        keep, lock, sound, win, BranchFrom, CaptureFailure, Corner, Motion, Overlay, OverlayConfig,
    };
    use crate::webview_memory::set_low_memory;
    use serde::{Deserialize, Serialize};
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use tauri::{
        AppHandle, Emitter, Listener, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
    };
    use tauri_plugin_log::log::{info, warn};
    use worldline_core::models::{Game, Version};
    use worldline_core::WorldlineCore;

    const LABEL: &str = "overlay";
    /// How often a showing popup checks that the game is still in front and in place.
    const POLL: Duration = Duration::from_millis(200);
    /// The longest exit animation is 620 ms; hide the window anyway if "done" never comes.
    const LEAVE_LIMIT: Duration = Duration::from_millis(1500);
    /// After the game first comes to the front, past splash screens and Steam's own overlay
    /// toast, which it would otherwise show under (handoff, "Events").
    const SESSION_DELAY: Duration = Duration::from_secs(5);
    /// The card's fold and fly take 880 ms; return to the game anyway if "landed" never comes.
    const LAND_LIMIT: Duration = Duration::from_millis(1500);
    const LAND_POLL: Duration = Duration::from_millis(16);

    /// What the webview renders (`OverlayShow`, `gui/src/ipc/events.ts`).
    #[derive(Serialize, Clone)]
    #[serde(rename_all = "camelCase")]
    struct Show {
        id: u64,
        swap: bool,
        #[serde(flatten)]
        detail: Detail,
        title: String,
        accent_hue: i32,
        corner: Corner,
        /// Only the mark. Never for warnings and errors.
        quiet: bool,
        motion: Motion,
        language: String,
        duration_ms: u64,
        /// The Kept popup the naming card flew into: already in place, and silent.
        landed: bool,
    }

    /// What the popup says, by kind.
    #[derive(Serialize, Clone)]
    #[serde(
        tag = "kind",
        rename_all = "camelCase",
        rename_all_fields = "camelCase"
    )]
    enum Detail {
        Checkpoint {
            version: i32,
        },
        NewTimeline {
            version: i32,
            from: BranchFrom,
        },
        SessionStart {
            /// The game's latest version, Unix ms; `None` when it has none.
            last_at: Option<i64>,
        },
        DeviceAhead {
            device: String,
            count: u32,
        },
        CaptureFailed {
            reason: CaptureFailure,
            /// A keep's version, captured but then not tagged; `None` when nothing was saved.
            version: Option<i32>,
        },
        Kept {
            version: i32,
            /// What the player typed in the naming card (O4).
            name: Option<String>,
        },
    }

    impl Detail {
        fn kind(&self) -> Kind {
            match self {
                Detail::Checkpoint { .. } => Kind::Checkpoint,
                Detail::NewTimeline { .. } => Kind::NewTimeline,
                Detail::SessionStart { .. } => Kind::SessionStart,
                Detail::DeviceAhead { .. } => Kind::DeviceAhead,
                Detail::CaptureFailed { .. } => Kind::CaptureFailed,
                Detail::Kept { .. } => Kind::Kept,
            }
        }
    }

    /// The naming card, for either window (`NamingCard`, `gui/src/ipc/events.ts`).
    #[derive(Serialize, Clone)]
    #[serde(rename_all = "camelCase")]
    struct Card {
        session: u64,
        game_id: i32,
        version_id: i32,
        version: i32,
        /// The name the checkpoint has: Esc and an empty field keep it.
        auto: String,
        title: String,
        accent_hue: i32,
        /// The corner the card flies into over the game.
        corner: Corner,
        motion: Motion,
        language: String,
    }

    /// The overlay's card closed (`naming-closed`).
    #[derive(Deserialize)]
    struct Closed {
        session: u64,
        how: How,
        name: String,
    }

    #[derive(Serialize, Clone)]
    struct Leave {
        id: u64,
        /// The game left the front, moved or was minimised: a quick fade, not the fold.
        fast: bool,
    }

    pub fn setup(app: &tauri::App) {
        app.manage(Overlay::default());
        // These run on the main thread, which must not wait for the popup lock.
        let handle = app.handle().clone();
        app.listen("overlay-ready", move |_| {
            let app = handle.clone();
            std::thread::spawn(move || lock(&app.state::<Overlay>().popup).set_ready());
        });
        let handle = app.handle().clone();
        app.listen("overlay-done", move |event| {
            let Ok(id) = serde_json::from_str::<u64>(event.payload()) else {
                return;
            };
            let app = handle.clone();
            std::thread::spawn(move || finish(&app, id));
        });
        let handle = app.handle().clone();
        app.listen("naming-closed", move |event| {
            let Ok(closed) = serde_json::from_str::<Closed>(event.payload()) else {
                return;
            };
            let app = handle.clone();
            std::thread::spawn(move || card_closed(&app, closed.session, closed.how, &closed.name));
        });
        let handle = app.handle().clone();
        app.listen("naming-landed", move |event| {
            let Ok(id) = serde_json::from_str::<u64>(event.payload()) else {
                return;
            };
            let app = handle.clone();
            std::thread::spawn(move || lock(&app.state::<Overlay>().naming).land(id));
        });
    }

    /// Creates the overlay window, hidden, and starts watching the foreground window when a game
    /// with an install dir starts. The game may already be in front.
    pub fn game_started(app: &AppHandle, game_id: i32) {
        let install_dir = app
            .state::<Arc<WorldlineCore>>()
            .get_game(game_id)
            .ok()
            .and_then(|game| game.game.install_dir);
        let overlay = app.state::<Overlay>();
        let mut games = lock(&overlay.games);
        if !games.started(game_id, install_dir.as_deref()) {
            return;
        }
        if app.get_webview_window(LABEL).is_none() {
            lock(&overlay.popup).reset();
            create(app);
        }
        let mut hook = lock(&overlay.hook);
        if hook.is_none() {
            let handle = app.clone();
            *hook = Some(win::on_foreground(move |hwnd| {
                foreground_changed(&handle, hwnd)
            }));
        }
        drop(hook);
        drop(games);
        foreground_changed(app, win::foreground());
    }

    /// Destroys the overlay window and stops watching the foreground window when the last game
    /// with an install dir stops.
    pub fn game_stopped(app: &AppHandle, game_id: i32) {
        let overlay = app.state::<Overlay>();
        // Its card over the game is asked for what was typed first, briefly.
        save_draft(app, game_id);
        let mut games = lock(&overlay.games);
        let last = games.stopped(game_id);
        // A press for that run, still on its way, is stale now.
        overlay.generation.advance();
        // Its card over the game goes with it, renamed already or named as the press kept it.
        if lock(&overlay.naming).abandon(game_id).is_some() && !last {
            if let Some(window) = app.get_webview_window(LABEL) {
                uncover(&window);
                hide(&window);
            }
        }
        if last {
            *lock(&overlay.hook) = None;
            lock(&overlay.popup).reset();
            if let Some(window) = app.get_webview_window(LABEL) {
                if let Err(err) = window.destroy() {
                    warn!("overlay: couldn't destroy the window: {err}");
                }
            }
        }
        drop(games);
        foreground_changed(app, win::foreground());
    }

    /// The game of a card still taking a name over it stopped: the webview closes the card as a
    /// focus loss, sending what was typed, which is renamed. Waits for that, `LEAVE_LIMIT` at most.
    fn save_draft(app: &AppHandle, game_id: i32) {
        let overlay = app.state::<Overlay>();
        let Some(id) = lock(&overlay.naming).taking_name(game_id) else {
            return;
        };
        if let Err(err) = app.emit_to(LABEL, "naming-leave", id) {
            warn!("overlay: couldn't close the naming card: {err}");
            return;
        }
        let started = Instant::now();
        while !lock(&overlay.naming).settled(id) && started.elapsed() < LEAVE_LIMIT {
            std::thread::sleep(LAND_POLL);
        }
    }

    /// A new window is in front: records which running game it belongs to, if any, and gives the
    /// hotkeys to that game or back to other apps.
    fn foreground_changed(app: &AppHandle, hwnd: isize) {
        focus_changed(app, hwnd);
        if front_changed(app, hwnd) {
            hotkeys::sync(app);
        }
    }

    /// The card over the game lost the focus to `hwnd`: it closes, saving what was typed, and the
    /// focus stays where it went. The webview says what was typed; if it never does, the name the
    /// press kept stays.
    fn focus_changed(app: &AppHandle, hwnd: isize) {
        let overlay = app.state::<Overlay>();
        let Some(id) = lock(&overlay.naming).focus_lost(win::root(hwnd)) else {
            return;
        };
        if let Err(err) = app.emit_to(LABEL, "naming-leave", id) {
            warn!("overlay: couldn't close the naming card: {err}");
        }
        let app = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(LEAVE_LIMIT);
            if lock(&app.state::<Overlay>().naming).is_open(id) {
                card_closed(&app, id, How::Blur, "");
            }
        });
    }

    /// Records the game run in front; true if that changed. A game's first time in front after it
    /// starts brings its session start.
    fn front_changed(app: &AppHandle, hwnd: isize) -> bool {
        let overlay = app.state::<Overlay>();
        let mut front = lock(&overlay.front);
        let exe = win::exe_of(hwnd);
        let mut games = lock(&overlay.games);
        let game = exe
            .and_then(|exe| games.running_from(&exe))
            .and_then(|game_id| Some((game_id, games.run(game_id)?)));
        if !hotkeys::set_front(&mut front, game, &overlay.generation) {
            return false;
        }
        if let Some((game_id, _)) = game {
            if let Some(run) = games.take_session(game_id) {
                let app = app.clone();
                std::thread::spawn(move || session_start(&app, game_id, run));
            }
        }
        true
    }

    /// The GUI pushed a new config, maybe with other hotkeys. Synced off the main thread, which
    /// the plugin waits for.
    pub fn config_changed(app: &AppHandle) {
        let app = app.clone();
        std::thread::spawn(move || hotkeys::sync(&app));
    }

    /// Settings' recorder: whether `accelerator` is free to use (`hotkeys::check`).
    pub fn check_shortcut(app: &AppHandle, accelerator: &str) -> Result<(), String> {
        hotkeys::check(app, accelerator)
    }

    /// A hotkey was pressed, on a thread of its own. It counts only for the game run that was in
    /// front when it was pressed, while that game is still in front and the hotkey still set, and
    /// not within the hotkey's cooldown from the press: an ignored press does nothing at all.
    pub fn pressed(app: &AppHandle, press: Press) {
        let overlay = app.state::<Overlay>();
        // Read with the generation under the front lock, which `set_front` moves it under: the
        // run in front when the press was made, or nothing.
        let front = {
            let front = lock(&overlay.front);
            front.filter(|_| overlay.generation.current() == press.generation)
        };
        let Some((game_id, run)) = front else {
            return;
        };
        let Some(config) = lock(&overlay.config).clone() else {
            return;
        };
        if !press.stands(&overlay.generation, &config)
            || lock(&overlay.games).install_dir(game_id, run).is_none()
        {
            return;
        }
        let Ok(full) = app.state::<Arc<WorldlineCore>>().get_game(game_id) else {
            return;
        };
        // The game may have left the front since the press. Its window, remembered for Name it
        // before anything else happens.
        let in_front = full
            .game
            .install_dir
            .as_deref()
            .and_then(win::foreground_from);
        let Some(game_window) = in_front else {
            return;
        };
        let template = &config.kept_tag_template;
        match press.action {
            Action::QuickKeep => {
                if lock(&overlay.cooldown).accept(press.action, press.at) {
                    quick_keep(app, full.game, press, template);
                }
            }
            Action::NameIt => {
                let accepted = hotkeys::accept_name_it(
                    &mut lock(&overlay.naming),
                    &mut lock(&overlay.cooldown),
                    press.at,
                );
                if accepted {
                    name_it(app, full.game, press, template, game_window);
                    // A card that opened holds it on its own; frees it if none did.
                    lock(&overlay.naming).end();
                }
            }
        }
    }

    /// Keeps the checkpoint of `game`, the game `press` was made for, and says so without taking
    /// focus: the Kept popup, or the capture-failed one, if it can show. With no popup possible,
    /// only its sound; a popup dropped under a warning or error, nothing.
    fn quick_keep(app: &AppHandle, game: Game, press: Press, template: &str) {
        let Some(result) = keep_for(app, &game, press, template) else {
            return;
        };
        let detail = match result {
            Ok(version) => Detail::Kept {
                version: version.version_number,
                name: None,
            },
            Err(not_kept) => not_kept_detail(not_kept),
        };
        feedback(app, game, detail);
    }

    /// Keeps the checkpoint of `game`, the game `press` was made for, then asks for its name:
    /// over the game when the card can show there, otherwise in Worldline. `game_window` is the
    /// game's window at the press. A failed keep only says so, as Quick keep does.
    fn name_it(app: &AppHandle, game: Game, press: Press, template: &str, game_window: isize) {
        let Some(result) = keep_for(app, &game, press, template) else {
            return;
        };
        let version = match result {
            Ok(version) => version,
            Err(not_kept) => return feedback(app, game, not_kept_detail(not_kept)),
        };
        let Some(config) = lock(&app.state::<Overlay>().config).clone() else {
            return;
        };
        // Kept: its sound, once, now. The popup that lands after the card stays silent.
        if config.sound {
            sound::play(Sound::Kept);
        }
        open_card(app, game, &version, game_window, &config);
    }

    fn not_kept_detail(not_kept: NotKept) -> Detail {
        Detail::CaptureFailed {
            reason: not_kept.reason,
            version: not_kept.captured,
        }
    }

    /// A hotkey's result: its popup if it can show, otherwise only its sound.
    fn feedback(app: &AppHandle, game: Game, detail: Detail) {
        let kind = detail.kind();
        let presented = present(app, game, detail, None, false);
        let sound_on = lock(&app.state::<Overlay>().config)
            .as_ref()
            .is_some_and(|config| config.sound);
        if let Some(sound) = decide::hotkey_sound(kind, presented, sound_on) {
            sound::play(sound);
        }
    }

    /// Keeps `game`'s checkpoint for `press`; `None` when the press went stale before the
    /// capture.
    fn keep_for(
        app: &AppHandle,
        game: &Game,
        press: Press,
        template: &str,
    ) -> Option<Result<Version, NotKept>> {
        let overlay = app.state::<Overlay>();
        let install_dir = game.install_dir.clone();
        // Asked again under the repository lock, right before the capture. No overlay lock but
        // the config's, which nothing holds while it waits.
        let still = || {
            let stands = lock(&overlay.config)
                .as_ref()
                .is_some_and(|config| press.stands(&overlay.generation, config));
            stands
                && install_dir
                    .as_deref()
                    .and_then(win::foreground_from)
                    .is_some()
        };
        let result = keep::keep(app, game.id, template, still);
        if result.is_none() {
            info!("overlay: a hotkey press for game {} went stale", game.id);
        }
        result
    }

    /// Opens the naming card for `version`, just kept: over the game if the popup is on for it,
    /// the game is still in front with the overlay ready, nothing runs in exclusive fullscreen,
    /// and the overlay gets the focus; otherwise in Worldline, brought to the front.
    fn open_card(
        app: &AppHandle,
        game: Game,
        version: &Version,
        game_window: isize,
        config: &OverlayConfig,
    ) {
        let overlay = app.state::<Overlay>();
        let corner = decide::corner(
            config.enabled,
            config.corner,
            config.per_game.get(&game.uuid).copied(),
        );
        let auto = version.tag.clone().unwrap_or_default();
        let opening = |place, was_tray| Opening {
            game_id: game.id,
            version_id: version.id,
            version_number: version.version_number,
            auto: auto.clone(),
            game_window,
            place,
            was_tray,
        };
        let card = |session| Card {
            session,
            game_id: game.id,
            version_id: version.id,
            version: version.version_number,
            auto: auto.clone(),
            title: game.title.clone(),
            accent_hue: game.accent_hue,
            corner: corner.unwrap_or(config.corner),
            motion: config.motion,
            language: config.language.clone(),
        };

        // The lifecycle lock, held until the card has the focus: no stop or restart in between.
        let games = lock(&overlay.games);
        let mut popup = lock(&overlay.popup);
        let mut naming = lock(&overlay.naming);
        let window = app.get_webview_window(LABEL);
        let client = win::client_rect(game_window);
        let in_front = game.install_dir.as_deref().and_then(win::foreground_from);
        let gate = in_front == Some(game_window)
            && popup.is_ready()
            && window.is_some()
            && client.is_some();
        let place = naming::place(corner.is_some(), gate, win::d3d_fullscreen(), || {
            let (Some(window), Some(client)) = (window.as_ref(), client) else {
                return false;
            };
            let Some(id) = naming.open(opening(Place::Overlay, false)).map(|s| s.id) else {
                return false;
            };
            popup.take_over();
            set_low_memory(window, false);
            if let Err(err) = app.emit_to(LABEL, "naming-open", card(id)) {
                warn!("overlay: couldn't send the naming card: {err}");
            }
            cover(window, client, game_window);
            let handle = win::handle(window);
            if !win::take_focus(handle) {
                info!("overlay: Windows kept the focus from the naming card; it opens in the app");
                naming.cancel(id);
                uncover(window);
                hide(window);
                return false;
            }
            // Into the webview too, as its WM_SETFOCUS does.
            if let Err(err) = window.as_ref().set_focus() {
                warn!("overlay: couldn't focus the naming card: {err}");
            }
            naming.focused(id, handle);
            true
        });
        drop(naming);
        drop(popup);
        drop(games);
        if place == Place::Overlay {
            return;
        }

        let Some(main) = app.get_webview_window("main") else {
            return;
        };
        // Hidden is in the tray; minimised isn't.
        let was_tray = !main.is_visible().unwrap_or(true);
        let Some(id) = lock(&overlay.naming)
            .open(opening(Place::App, was_tray))
            .map(|s| s.id)
        else {
            return;
        };
        crate::tray::show_window(app);
        if let Err(err) = app.emit_to("main", "name-it-open", card(id)) {
            warn!("overlay: couldn't send the naming card to the app: {err}");
        }
    }

    /// The overlay over the game's whole client area, directly above the game, taking clicks and
    /// able to take the focus: for the card. Shown, but not activated yet.
    fn cover(window: &WebviewWindow, client: Rect, game: isize) {
        win::interactive(window, true);
        win::show_at(window, client, game);
    }

    /// The overlay back to the popup's: click-through, and never activated.
    fn uncover(window: &WebviewWindow) {
        win::interactive(window, false);
    }

    /// Renames the card's checkpoint to the name it closed with, only if that isn't its name
    /// already (`tag_version`, under the repository lock). The name it ends up with, or why it
    /// couldn't be renamed: it keeps the name the press gave it then.
    fn rename(app: &AppHandle, session: &Session, how: How, typed: &str) -> Result<String, String> {
        let name = naming::final_name(how, typed, &session.auto);
        let Some(changed) = naming::rename(Some(&session.auto), &name) else {
            return Ok(name);
        };
        let core = app.state::<Arc<WorldlineCore>>();
        match core.tag_version(session.version_id, changed.to_owned()) {
            Ok(_) => {
                app.emit("version-kept", (session.game_id, session.version_id))
                    .ok();
                Ok(name)
            }
            Err(err) => {
                warn!(
                    "overlay: couldn't name version {}: {err:#}",
                    session.version_id
                );
                Err(format!("{err:#}"))
            }
        }
    }

    /// The card over the game closed (`naming-closed`), or its webview never said: renames the
    /// checkpoint, then, once the card has flown into its corner, returns to the game.
    fn card_closed(app: &AppHandle, id: u64, how: How, typed: &str) {
        let overlay = app.state::<Overlay>();
        let Some(session) = lock(&overlay.naming).close(id, Place::Overlay, how) else {
            return;
        };
        // Unrenamed, it keeps the press's name: the Kept popup then shows no name, which is true.
        let name = rename(app, &session, how, typed).unwrap_or_else(|_| session.auto.clone());
        lock(&overlay.naming).named(id, name);
        let started = Instant::now();
        while lock(&overlay.naming).folding(id) && started.elapsed() < LAND_LIMIT {
            std::thread::sleep(LAND_POLL);
        }
        card_return(app, id);
    }

    /// After card `id` over the game: the focus back to the game only when it was closed
    /// explicitly, the window passive and click-through again, and the Kept popup, already in its
    /// corner and silent, or no window at all. The card is only gone once the window is passive,
    /// so no newer card can open before. Nothing if its game stopped meanwhile.
    fn card_return(app: &AppHandle, id: u64) {
        let overlay = app.state::<Overlay>();
        let released = release(&overlay, id, |session, how| {
            let Some(window) = app.get_webview_window(LABEL) else {
                return false;
            };
            if how.hands_back() && !win::hand_back(session.game_window) {
                info!("overlay: couldn't give the focus back to the game");
            }
            uncover(&window);
            true
        });
        let Some((session, name)) = released else {
            return;
        };
        let detail = Detail::Kept {
            version: session.version_number,
            name: (name != session.auto).then_some(name),
        };
        let shown = match app.state::<Arc<WorldlineCore>>().get_game(session.game_id) {
            Ok(full) => present(app, full.game, detail, None, true) == Presented::Shown,
            Err(_) => false,
        };
        if !shown {
            // Unless another popup showed, or another card opened, meanwhile.
            let _games = lock(&overlay.games);
            let popup = lock(&overlay.popup);
            let idle = popup.is_hidden() && !lock(&overlay.naming).over_game();
            if let (true, Some(window)) = (idle, app.get_webview_window(LABEL)) {
                hide(&window);
            }
        }
    }

    /// Ends card `id` over the game, `cleanup` (its hand-back, the window passive again) done
    /// under the lifecycle lock, which opening a card, a stop and a restart all take: none of
    /// them happens in between. Its session and name; `None` if it had ended already, or
    /// `cleanup` found no window.
    fn release(
        overlay: &Overlay,
        id: u64,
        cleanup: impl FnOnce(&Session, How) -> bool,
    ) -> Option<(Session, String)> {
        let _games = lock(&overlay.games);
        let (session, how, name) = lock(&overlay.naming).finish(id)?;
        cleanup(&session, how).then_some((session, name))
    }

    /// The card in the app closed (`name_it_closed`): renames the checkpoint and, after an
    /// explicit close, gives the focus back to the game if its window is still there, hiding
    /// Worldline to the tray again if it came from there and the name was saved, so a failure is
    /// seen.
    pub fn name_it_closed(app: &AppHandle, id: u64, how: How, typed: &str) -> NameItClosed {
        let overlay = app.state::<Overlay>();
        let Some(session) = lock(&overlay.naming).close(id, Place::App, how) else {
            return NameItClosed::default();
        };
        let error = rename(app, &session, how, typed).err();
        let (hand_back, rehide) = naming::app_return(
            how,
            win::exists(session.game_window),
            session.was_tray,
            error.is_none(),
        );
        let handed = hand_back && win::hand_back(session.game_window);
        if handed && rehide {
            if let Some(main) = app.get_webview_window("main") {
                crate::tray::hide_window(&main);
            }
        }
        NameItClosed { handed, error }
    }

    /// The session start of the game's run `run`, or the device-ahead warning in its place. Not
    /// if the run ended meanwhile: checked early, and again when the popup is offered.
    fn session_start(app: &AppHandle, game_id: i32, run: u64) {
        std::thread::sleep(SESSION_DELAY);
        let overlay = app.state::<Overlay>();
        if lock(&overlay.games).install_dir(game_id, run).is_none() {
            return;
        }
        let warn_ahead = lock(&overlay.config)
            .as_ref()
            .is_some_and(|config| config.warn_ahead);
        let core = app.state::<Arc<WorldlineCore>>();
        let cloud_on = core
            .get_sync_settings()
            .is_ok_and(|settings| settings.remote_path.is_some());
        // The sync status as it is now, read from the remote without any lock held. Unreadable
        // counts as unknown: the plain session start.
        let synced = if warn_ahead && cloud_on {
            core.get_game(game_id)
                .ok()
                .and_then(|full| uuid::Uuid::parse_str(&full.game.uuid).ok())
                .and_then(|uuid| core.game_sync_status(uuid).ok().flatten())
        } else {
            None
        };
        // After the status, so the count runs from where this device is now.
        let Ok(full) = core.get_game(game_id) else {
            return;
        };
        let ahead = synced.as_ref().and_then(|(status, parents)| {
            let local = full
                .last_version
                .as_ref()
                .and_then(|version| uuid::Uuid::parse_str(&version.uuid).ok());
            decide::device_ahead(warn_ahead, cloud_on, Some(status), parents, local)
        });
        let detail = match ahead {
            Some(ahead) => Detail::DeviceAhead {
                device: ahead.device,
                count: ahead.count,
            },
            None => Detail::SessionStart {
                last_at: full
                    .versions
                    .iter()
                    .map(|version| version.created_at)
                    .max()
                    .map(|at| at.and_utc().timestamp_millis()),
            },
        };
        present(app, full.game, detail, Some(run), false);
    }

    fn create(app: &AppHandle) {
        let (w, h) = (
            decide::POPUP.0 + 2.0 * decide::MARGIN,
            decide::POPUP.1 + 2.0 * decide::MARGIN,
        );
        let built = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("overlay.html".into()))
            .title("Worldline overlay")
            .transparent(true)
            .decorations(false)
            .skip_taskbar(true)
            .always_on_top(false)
            .focused(false)
            .focusable(false)
            .shadow(false)
            .resizable(false)
            .visible(false)
            .inner_size(w, h)
            .build();
        let window = match built {
            Ok(window) => window,
            Err(err) => {
                warn!("overlay: couldn't create the window: {err}");
                return;
            }
        };
        // Clicks go through to the game; then the styles Tauri doesn't set.
        if let Err(err) = window.set_ignore_cursor_events(true) {
            warn!("overlay: clicks won't go through: {err}");
        }
        win::passive(&window);
        set_low_memory(&window, true);
    }

    /// The checkpoint popup, or the new-timeline one when the version starts a new timeline.
    pub fn version_created(
        app: &AppHandle,
        game_id: i32,
        version_number: i32,
        branch_from: Option<BranchFrom>,
    ) {
        let Ok(full) = app.state::<Arc<WorldlineCore>>().get_game(game_id) else {
            return;
        };
        let detail = match branch_from {
            Some(from) => Detail::NewTimeline {
                version: version_number,
                from,
            },
            None => Detail::Checkpoint {
                version: version_number,
            },
        };
        present(app, full.game, detail, None, false);
    }

    /// An automatic capture failed: nothing was saved.
    pub fn checkpoint_failed(app: &AppHandle, game_id: i32, reason: CaptureFailure) {
        let Ok(full) = app.state::<Arc<WorldlineCore>>().get_game(game_id) else {
            return;
        };
        let detail = Detail::CaptureFailed {
            reason,
            version: None,
        };
        present(app, full.game, detail, None, false);
    }

    /// Shows the popup if `game` is the window in front; otherwise it's dropped, not queued.
    /// `run` names the run a session start belongs to; it's dropped if that run has ended.
    /// Whether it showed, with its sound, was turned away, or couldn't show at all. `landed`: the
    /// Kept popup the naming card flew into, without its rise or sound. Nothing shows while the
    /// card is over the game.
    fn present(
        app: &AppHandle,
        game: Game,
        detail: Detail,
        run: Option<u64>,
        landed: bool,
    ) -> Presented {
        let overlay = app.state::<Overlay>();
        let Some(config) = lock(&overlay.config).clone() else {
            return Presented::Unavailable;
        };
        let Some(install_dir) = game.install_dir.as_deref() else {
            return Presented::Unavailable;
        };
        let corner = decide::corner(
            config.enabled,
            config.corner,
            config.per_game.get(&game.uuid).copied(),
        );
        let Some(corner) = corner else {
            return Presented::Unavailable;
        };
        // Only over the game, only while it's in front; dropped otherwise, not queued.
        let Some(hwnd) = win::foreground_from(install_dir) else {
            info!("overlay: game {} isn't in front; no popup", game.id);
            return Presented::Unavailable;
        };
        let Some(client) = win::client_rect(hwnd) else {
            return Presented::Unavailable;
        };
        let rect = decide::place(client, decide::POPUP, win::scale(hwnd), corner);

        let kind = detail.kind();
        // The lifecycle lock, held until the popup shows: no stop or restart in between.
        let games = lock(&overlay.games);
        let mut popup = lock(&overlay.popup);
        let Some(window) = app.get_webview_window(LABEL) else {
            return Presented::Unavailable;
        };
        if !popup.is_ready() {
            return Presented::Unavailable;
        }
        if lock(&overlay.naming).over_game() {
            return Presented::Dropped;
        }
        let session = run.map(|run| (game.id, run));
        let (outcome, id) = decide::admit(&games, &mut popup, kind, session, config.quiet);
        if !matches!(outcome, Outcome::Show | Outcome::Swap) {
            return outcome.into();
        }
        let duration = kind.duration(config.quiet);
        set_low_memory(&window, false);
        let show = Show {
            id,
            swap: outcome == Outcome::Swap,
            detail,
            title: game.title,
            accent_hue: game.accent_hue,
            corner,
            quiet: config.quiet && !kind.urgent(),
            motion: config.motion,
            language: config.language,
            duration_ms: duration.as_millis() as u64,
            landed,
        };
        if let Err(err) = app.emit_to(LABEL, "overlay-show", show) {
            warn!("overlay: couldn't send the popup: {err}");
        }
        win::show_at(&window, rect, hwnd);
        if config.sound && !landed {
            if let Some(sound) = decide::sound(kind, outcome) {
                sound::play(sound);
            }
        }
        drop(popup);
        drop(games);

        let app = app.clone();
        std::thread::spawn(move || watch(&app, id, hwnd, client, duration));
        Presented::Shown
    }

    /// Keeps popup `id` above the game until its time is up, or at once when the game leaves the
    /// front, moves, resizes or minimises; then asks the webview to play the exit. Keeps looking
    /// until the window hides: the game leaving during the fold still gets the quick fade.
    fn watch(app: &AppHandle, id: u64, game: isize, client: Rect, duration: Duration) {
        let overlay = app.state::<Overlay>();
        let started = Instant::now();
        let mut left: Option<Instant> = None;
        loop {
            std::thread::sleep(POLL);
            let mut popup = lock(&overlay.popup);
            let Some(window) = app.get_webview_window(LABEL) else {
                return;
            };
            let away = win::foreground() != game || win::client_rect(game) != Some(client);
            let due = started.elapsed() >= duration;
            let late = left.is_some_and(|at| at.elapsed() >= LEAVE_LIMIT);
            match popup.watch(id, away, due, late) {
                // The game can rise over the overlay: it isn't topmost over the game.
                Watch::Keep => win::restack(&window, game),
                Watch::Leave { fast } => {
                    left.get_or_insert_with(Instant::now);
                    if let Err(err) = app.emit_to(LABEL, "overlay-leave", Leave { id, fast }) {
                        warn!("overlay: couldn't end the popup: {err}");
                    }
                }
                Watch::Hide => {
                    hide(&window);
                    return;
                }
                // A newer popup took over, the exit ended, or the window went away.
                Watch::Stop => return,
            }
        }
    }

    /// Hides the window after popup `id`'s exit, unless a newer popup owns it.
    fn finish(app: &AppHandle, id: u64) {
        let overlay = app.state::<Overlay>();
        let mut popup = lock(&overlay.popup);
        if !popup.finish(id) {
            return;
        }
        if let Some(window) = app.get_webview_window(LABEL) {
            hide(&window);
        }
    }

    fn hide(window: &WebviewWindow) {
        win::hide(window);
        set_low_memory(window, true);
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn try_open(overlay: &Overlay) -> Option<u64> {
            // Opening a card takes the lifecycle lock first (`open_card`).
            let _games = overlay.games.try_lock().ok()?;
            let opening = Opening {
                game_id: 7,
                version_id: 41,
                version_number: 13,
                auto: "Auto".into(),
                game_window: 0x1234,
                place: Place::Overlay,
                was_tray: false,
            };
            lock(&overlay.naming).open(opening).map(|s| s.id)
        }

        #[test]
        fn an_older_cards_late_return_never_reaches_a_newer_card() {
            let overlay = Overlay::default();
            let older = try_open(&overlay).unwrap();
            {
                let mut naming = lock(&overlay.naming);
                naming.close(older, Place::Overlay, How::Save).unwrap();
                naming.named(older, "Boss".into());
            }
            let mut newer_meanwhile = None;
            let released = release(&overlay, older, |session, how| {
                assert_eq!((session.id, how), (older, How::Save));
                // A newer press opens its card, from its own thread, while the older card hands
                // the focus back and makes the window passive.
                newer_meanwhile =
                    std::thread::scope(|s| s.spawn(|| try_open(&overlay)).join().unwrap());
                true
            });
            assert_eq!(released.unwrap().1, "Boss");
            assert_eq!(
                newer_meanwhile, None,
                "a newer card opened before the older one's window was passive"
            );
            let newer = try_open(&overlay).expect("the card is free once the older one is done");
            // The older card's return, close or landing, late, leaves the newer card alone.
            let late = release(&overlay, older, |_, _| panic!("cleaned up the newer card"));
            assert!(late.is_none());
            let mut naming = lock(&overlay.naming);
            assert!(naming.close(older, Place::Overlay, How::Blur).is_none());
            naming.land(older);
            assert!(naming.is_open(newer) && naming.over_game());
        }
    }
}

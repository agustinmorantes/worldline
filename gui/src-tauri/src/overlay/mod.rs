//! The game overlay (ADR 0009): a small popup drawn over the game when something happens to its
//! saves, in a window of its own. Everything is decided here: the gate, the popup state, timers,
//! placement and sound. The overlay webview (`gui/overlay.html`) only renders what it's sent,
//! and says when its exit animation has ended.
//!
//! Windows only; elsewhere every entry point is a no-op. The hotkeys (`hotkeys.rs`) follow the
//! game in front and share the popup.
//!
//! Never take `hotkeys`, `front`, `games` or `popup` on the main thread: window calls from other
//! threads wait for the main thread, so a lock held there would deadlock them. Take them in that
//! order.

#[cfg_attr(not(windows), allow(dead_code))]
mod decide;
#[cfg(windows)]
mod hotkeys;
#[cfg(windows)]
mod keep;
#[cfg(windows)]
mod sound;
#[cfg(windows)]
mod win;

use decide::{Corner, GameCorner};
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
    /// Name it (O4).
    #[allow(dead_code)]
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
    check_shortcut, checkpoint_failed, config_changed, game_started, game_stopped, setup,
    version_created,
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

/// No hotkeys here, so none is taken.
#[cfg(not(windows))]
pub fn check_shortcut(_app: &tauri::AppHandle, _accelerator: &str) -> Result<(), String> {
    Ok(())
}

#[cfg(windows)]
mod windows {
    use super::decide::{self, Kind, Outcome, Presented, Rect, Watch};
    use super::hotkeys::{self, Action, Press};
    use super::{keep, lock, sound, win, BranchFrom, CaptureFailure, Corner, Motion, Overlay};
    use crate::webview_memory::set_low_memory;
    use serde::Serialize;
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use tauri::{
        AppHandle, Emitter, Listener, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
    };
    use tauri_plugin_log::log::{info, warn};
    use worldline_core::models::Game;
    use worldline_core::WorldlineCore;

    const LABEL: &str = "overlay";
    /// How often a showing popup checks that the game is still in front and in place.
    const POLL: Duration = Duration::from_millis(200);
    /// The longest exit animation is 620 ms; hide the window anyway if "done" never comes.
    const LEAVE_LIMIT: Duration = Duration::from_millis(1500);
    /// After the game first comes to the front, past splash screens (handoff, "Events").
    const SESSION_DELAY: Duration = Duration::from_secs(2);

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
        let mut games = lock(&overlay.games);
        let last = games.stopped(game_id);
        // A press for that run, still on its way, is stale now.
        overlay.generation.advance();
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

    /// A new window is in front: records which running game it belongs to, if any, and gives the
    /// hotkeys to that game or back to other apps.
    fn foreground_changed(app: &AppHandle, hwnd: isize) {
        if front_changed(app, hwnd) {
            hotkeys::sync(app);
        }
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
        // The game may have left the front since the press.
        let in_front = full
            .game
            .install_dir
            .as_deref()
            .and_then(win::foreground_from);
        if in_front.is_none() || !lock(&overlay.cooldown).accept(press.action, press.at) {
            return;
        }
        match press.action {
            Action::QuickKeep => quick_keep(app, full.game, press, &config.kept_tag_template),
        }
    }

    /// Keeps the checkpoint of `game`, the game `press` was made for, and says so without taking
    /// focus: the Kept popup, or the capture-failed one, if it can show. With no popup possible,
    /// only its sound; a popup dropped under a warning or error, nothing.
    fn quick_keep(app: &AppHandle, game: Game, press: Press, template: &str) {
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
        let Some(result) = keep::keep(app, game.id, template, still) else {
            info!("overlay: a hotkey press for game {} went stale", game.id);
            return;
        };
        let detail = match result {
            Ok(version) => Detail::Kept {
                version: version.version_number,
                name: None,
            },
            Err(not_kept) => Detail::CaptureFailed {
                reason: not_kept.reason,
                version: not_kept.captured,
            },
        };
        let kind = detail.kind();
        let presented = present(app, game, detail, None);
        let sound_on = lock(&overlay.config)
            .as_ref()
            .is_some_and(|config| config.sound);
        if let Some(sound) = decide::hotkey_sound(kind, presented, sound_on) {
            sound::play(sound);
        }
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
        present(app, full.game, detail, Some(run));
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
        present(app, full.game, detail, None);
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
        present(app, full.game, detail, None);
    }

    /// Shows the popup if `game` is the window in front; otherwise it's dropped, not queued.
    /// `run` names the run a session start belongs to; it's dropped if that run has ended.
    /// Whether it showed, with its sound, was turned away, or couldn't show at all.
    fn present(app: &AppHandle, game: Game, detail: Detail, run: Option<u64>) -> Presented {
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
        };
        if let Err(err) = app.emit_to(LABEL, "overlay-show", show) {
            warn!("overlay: couldn't send the popup: {err}");
        }
        win::show_at(&window, rect, hwnd);
        if config.sound {
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
}

//! The game overlay (ADR 0009): a small popup drawn over the game when something happens to its
//! saves, in a window of its own. Everything is decided here: the gate, the popup state, timers,
//! placement and sound. The overlay webview (`gui/overlay.html`) only renders what it's sent,
//! and says when its exit animation has ended.
//!
//! Windows only; elsewhere every entry point is a no-op. O1 shows checkpoints; the other events
//! (O2) and the hotkeys (O3, O4) build on the same popup.
//!
//! Never take `games` or `popup` on the main thread: window calls from other threads wait for
//! the main thread, so a lock held there would deadlock them.

#[cfg_attr(not(windows), allow(dead_code))]
mod decide;
#[cfg(windows)]
mod sound;
#[cfg(windows)]
mod win;

use decide::{Corner, GameCorner};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

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
    /// The device-ahead warning (O2) and the hotkeys (O3, O4).
    #[allow(dead_code)]
    warn_ahead: bool,
    #[allow(dead_code)]
    quick_keep_key: String,
    #[allow(dead_code)]
    name_it_key: String,
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
    /// Also the lifecycle lock: the window is only created or destroyed while it's held.
    #[cfg_attr(not(windows), allow(dead_code))]
    games: Mutex<decide::Games>,
    #[cfg_attr(not(windows), allow(dead_code))]
    popup: Mutex<decide::Popup>,
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
pub use windows::{game_started, game_stopped, setup, version_created};

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
pub fn version_created(_app: &tauri::AppHandle, _game_id: i32, _version_number: i32) {}

#[cfg(windows)]
mod windows {
    use super::decide::{self, Kind, Outcome, Rect, Watch};
    use super::{lock, sound, win, Corner, Motion, Overlay};
    use crate::webview_memory::set_low_memory;
    use serde::Serialize;
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use tauri::{
        AppHandle, Emitter, Listener, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
    };
    use tauri_plugin_log::log::{info, warn};
    use worldline_core::WorldlineCore;

    const LABEL: &str = "overlay";
    /// How often a showing popup checks that the game is still in front and in place.
    const POLL: Duration = Duration::from_millis(200);
    /// The longest exit animation is 620 ms; hide the window anyway if "done" never comes.
    const LEAVE_LIMIT: Duration = Duration::from_millis(1500);

    /// What the webview renders (`OverlayShow`, `gui/src/ipc/events.ts`).
    #[derive(Serialize, Clone)]
    #[serde(rename_all = "camelCase")]
    struct Show {
        id: u64,
        swap: bool,
        kind: Kind,
        title: String,
        accent_hue: i32,
        version: i32,
        corner: Corner,
        quiet: bool,
        motion: Motion,
        language: String,
        duration_ms: u64,
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

    /// Creates the overlay window, hidden, when a game with an install dir starts.
    pub fn game_started(app: &AppHandle, game_id: i32) {
        let has_install_dir = app
            .state::<Arc<WorldlineCore>>()
            .get_game(game_id)
            .is_ok_and(|game| game.game.install_dir.is_some());
        let overlay = app.state::<Overlay>();
        let mut games = lock(&overlay.games);
        if games.started(game_id, has_install_dir) && app.get_webview_window(LABEL).is_none() {
            lock(&overlay.popup).reset();
            create(app);
        }
    }

    /// Destroys the overlay window when the last game with an install dir stops.
    pub fn game_stopped(app: &AppHandle, game_id: i32) {
        let overlay = app.state::<Overlay>();
        let mut games = lock(&overlay.games);
        if games.stopped(game_id) {
            lock(&overlay.popup).reset();
            if let Some(window) = app.get_webview_window(LABEL) {
                if let Err(err) = window.destroy() {
                    warn!("overlay: couldn't destroy the window: {err}");
                }
            }
        }
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

    /// Shows the checkpoint popup if the game that saved is the window in front.
    pub fn version_created(app: &AppHandle, game_id: i32, version_number: i32) {
        let overlay = app.state::<Overlay>();
        let Some(config) = lock(&overlay.config).clone() else {
            return;
        };
        let Ok(full) = app.state::<Arc<WorldlineCore>>().get_game(game_id) else {
            return;
        };
        let game = full.game;
        let Some(install_dir) = game.install_dir.as_deref() else {
            return;
        };
        let corner = decide::corner(
            config.enabled,
            config.corner,
            config.per_game.get(&game.uuid).copied(),
        );
        let Some(corner) = corner else {
            return;
        };
        // Only over the game, only while it's in front; dropped otherwise, not queued.
        let Some(hwnd) = win::foreground_from(install_dir) else {
            info!("overlay: game {game_id} isn't in front; no popup");
            return;
        };
        let Some(client) = win::client_rect(hwnd) else {
            return;
        };
        let rect = decide::place(client, decide::POPUP, win::scale(hwnd), corner);

        let kind = Kind::Checkpoint;
        let mut popup = lock(&overlay.popup);
        let Some(window) = app.get_webview_window(LABEL) else {
            return;
        };
        let (outcome, id) = popup.offer(kind);
        if outcome == Outcome::Drop {
            return;
        }
        let duration = kind.duration(config.quiet);
        set_low_memory(&window, false);
        let show = Show {
            id,
            swap: outcome == Outcome::Swap,
            kind,
            title: game.title,
            accent_hue: game.accent_hue,
            version: version_number,
            corner,
            quiet: config.quiet,
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

        let app = app.clone();
        std::thread::spawn(move || watch(&app, id, hwnd, client, duration));
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

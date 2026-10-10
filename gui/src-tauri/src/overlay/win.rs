//! The overlay's Win32 side, as S0 measured it: the foreground gate, the client area and DPI,
//! and showing the window without activating it, directly above the game in the z-order. Also
//! the hook that reports each new foreground window. For the naming card (O4), the overlay over
//! the whole client area, taking the focus, and the focus handed back to the game.

use super::decide::Rect;
use std::cell::RefCell;
use std::os::windows::ffi::OsStringExt;
use tauri::WebviewWindow;
use tauri_plugin_log::log::warn;
use windows_sys::Win32::Foundation::{CloseHandle, HWND, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    ClientToScreen, MonitorFromWindow, MONITOR_DEFAULTTONEAREST,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentThreadId, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
};
use windows_sys::Win32::UI::Shell::{SHQueryUserNotificationState, QUNS_RUNNING_D3D_FULL_SCREEN};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

fn hwnd(window: &WebviewWindow) -> HWND {
    window.hwnd().map_or(std::ptr::null_mut(), |h| h.0 as HWND)
}

pub fn foreground() -> isize {
    unsafe { GetForegroundWindow() as isize }
}

type Changed = Box<dyn Fn(isize)>;

thread_local! {
    /// The hook thread's callback: the hook procedure gets no user data.
    static CHANGED: RefCell<Option<Changed>> = const { RefCell::new(None) };
}

/// The foreground hook's thread. Dropping it ends the thread and unhooks.
pub struct ForegroundHook(u32);

impl Drop for ForegroundHook {
    fn drop(&mut self) {
        unsafe { PostThreadMessageW(self.0, WM_QUIT, 0, 0) };
    }
}

/// Calls `changed` with every new foreground window, on a thread of its own with a message loop.
/// The hook is out of context: Windows calls it in our process, nothing goes into the game's.
pub fn on_foreground(changed: impl Fn(isize) + Send + 'static) -> ForegroundHook {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        CHANGED.with(|cell| *cell.borrow_mut() = Some(Box::new(changed)));
        unsafe {
            let mut msg: MSG = std::mem::zeroed();
            // Creates the thread's queue, so the quit can't be posted before it exists.
            PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_NOREMOVE);
            let _ = tx.send(GetCurrentThreadId());
            let hook = SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                std::ptr::null_mut(),
                Some(foreground_changed),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            );
            if hook.is_null() {
                warn!("overlay: couldn't watch the foreground window");
                return;
            }
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                DispatchMessageW(&msg);
            }
            UnhookWinEvent(hook);
        }
    });
    ForegroundHook(rx.recv().unwrap_or(0))
}

unsafe extern "system" fn foreground_changed(
    _hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    object: i32,
    child: i32,
    _thread: u32,
    _time: u32,
) {
    if object == OBJID_WINDOW && child == CHILDID_SELF as i32 {
        CHANGED.with(|cell| {
            if let Some(changed) = cell.borrow().as_ref() {
                changed(hwnd as isize);
            }
        });
    }
}

/// The executable of the process that owns `hwnd`, or `None` if it's no longer a window.
pub fn exe_of(hwnd: isize) -> Option<String> {
    let mut pid = 0u32;
    if unsafe { GetWindowThreadProcessId(hwnd as HWND, &mut pid) } == 0 {
        return None;
    }
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return None;
    }
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    let ok = unsafe {
        QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len)
    };
    unsafe { CloseHandle(process) };
    (ok != 0).then(|| {
        std::ffi::OsString::from_wide(&buf[..len as usize])
            .to_string_lossy()
            .into_owned()
    })
}

/// The foreground window, if its process runs from `install_dir` (ADR 0009's gate).
pub fn foreground_from(install_dir: &str) -> Option<isize> {
    let hwnd = foreground();
    let exe = exe_of(hwnd)?;
    worldline_core::detection::runs_from(std::path::Path::new(&exe), install_dir).then_some(hwnd)
}

/// The window's client area in screen pixels, or `None` while it's minimised.
pub fn client_rect(hwnd: isize) -> Option<Rect> {
    if unsafe { IsIconic(hwnd as HWND) } != 0 {
        return None;
    }
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    let mut origin = POINT { x: 0, y: 0 };
    unsafe {
        if GetClientRect(hwnd as HWND, &mut rect) == 0
            || ClientToScreen(hwnd as HWND, &mut origin) == 0
        {
            return None;
        }
    }
    Some(Rect {
        x: origin.x,
        y: origin.y,
        w: rect.right,
        h: rect.bottom,
    })
}

/// The DPI scale of the monitor the window is on now.
pub fn scale(hwnd: isize) -> f64 {
    let (mut x, mut y) = (96u32, 96u32);
    unsafe {
        let monitor = MonitorFromWindow(hwnd as HWND, MONITOR_DEFAULTTONEAREST);
        GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut x, &mut y);
    }
    x as f64 / 96.0
}

/// Keeps the overlay out of Alt+Tab and the taskbar, and never activated. Call it after Tauri's
/// own style changes, which rewrite these bits (S0).
pub fn passive(window: &WebviewWindow) {
    let hwnd = hwnd(window);
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let style =
            (style | (WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE) as isize) & !(WS_EX_APPWINDOW as isize);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style);
        let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED;
        SetWindowPos(hwnd, std::ptr::null_mut(), 0, 0, 0, 0, flags);
    }
}

fn topmost(hwnd: HWND) -> bool {
    unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST != 0 }
}

/// Where the overlay goes in the z-order: directly above `game`, in the game's band (topmost only
/// if the game is), so windows in front of the game cover the overlay too. `None` when it's
/// there already.
fn above(overlay: HWND, game: HWND) -> Option<HWND> {
    unsafe {
        if topmost(overlay) && !topmost(game) {
            let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
            SetWindowPos(overlay, HWND_NOTOPMOST, 0, 0, 0, 0, flags);
        }
        let prev = GetWindow(game, GW_HWNDPREV);
        if prev == overlay {
            None
        } else if prev.is_null() || topmost(prev) != topmost(game) {
            // The game heads its band: go to the top of that band.
            Some(if topmost(game) {
                HWND_TOPMOST
            } else {
                HWND_NOTOPMOST
            })
        } else {
            // Behind the window right above the game, which is just above the game.
            Some(prev)
        }
    }
}

/// Puts the overlay back directly above `game` if the game rose over it.
pub fn restack(window: &WebviewWindow, game: isize) {
    let hwnd = hwnd(window);
    if let Some(after) = above(hwnd, game as HWND) {
        let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE;
        unsafe { SetWindowPos(hwnd, after, 0, 0, 0, 0, flags) };
    }
}

/// Moves the overlay to `rect`, directly above `game`, and shows it without activating it.
pub fn show_at(window: &WebviewWindow, rect: Rect, game: isize) {
    let hwnd = hwnd(window);
    let (after, z) = match above(hwnd, game as HWND) {
        Some(after) => (after, 0),
        None => (std::ptr::null_mut(), SWP_NOZORDER),
    };
    let flags = SWP_NOACTIVATE | z;
    unsafe {
        SetWindowPos(hwnd, after, rect.x, rect.y, rect.w, rect.h, flags);
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
}

pub fn hide(window: &WebviewWindow) {
    unsafe { ShowWindow(hwnd(window), SW_HIDE) };
}

pub fn handle(window: &WebviewWindow) -> isize {
    hwnd(window) as isize
}

/// The top-level window `hwnd` belongs to.
pub fn root(hwnd: isize) -> isize {
    unsafe { GetAncestor(hwnd as HWND, GA_ROOT) as isize }
}

/// Windows reports a Direct3D app in exclusive fullscreen: nothing can show over it.
pub fn d3d_fullscreen() -> bool {
    let mut state = 0;
    unsafe {
        SHQueryUserNotificationState(&mut state) == 0 && state == QUNS_RUNNING_D3D_FULL_SCREEN
    }
}

/// For the naming card, lets the overlay take clicks and be activated; or back to the popup's
/// click-through, never activated. Still out of Alt+Tab and the taskbar either way. Win32 only:
/// Tauri's own setters apply later, on the main thread, and would undo `passive`.
pub fn interactive(window: &WebviewWindow, on: bool) {
    let hwnd = hwnd(window);
    let bits = (WS_EX_NOACTIVATE | WS_EX_TRANSPARENT) as isize;
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let style = if on { style & !bits } else { style | bits };
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style);
        let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED;
        SetWindowPos(hwnd, std::ptr::null_mut(), 0, 0, 0, 0, flags);
    }
}

/// Brings `hwnd` to the front with the focus; true if it's the foreground window now. Windows
/// often refuses a background process that, so when it does, an injected key press and release
/// lifts the foreground lock and it's tried once more. tao's `set_focus` injects Alt (S0); this
/// injects an unassigned key instead, which no game or layout binds.
pub fn take_focus(hwnd: isize) -> bool {
    let fronted = || unsafe {
        SetForegroundWindow(hwnd as HWND) != 0 && GetForegroundWindow() == hwnd as HWND
    };
    fronted() || {
        tap_unassigned_key();
        fronted()
    }
}

/// Virtual-key 0xE8, listed as unassigned by Windows.
const VK_UNASSIGNED: u16 = 0xE8;

fn tap_unassigned_key() {
    let key = |flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VK_UNASSIGNED,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let inputs = [key(0), key(KEYEVENTF_KEYUP)];
    unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        );
    }
}

/// Whether `hwnd` is still a window.
pub fn exists(hwnd: isize) -> bool {
    unsafe { IsWindow(hwnd as HWND) != 0 }
}

/// Gives the focus back to the game's window, restoring it first if it's minimised.
pub fn hand_back(hwnd: isize) -> bool {
    unsafe {
        if IsIconic(hwnd as HWND) != 0 {
            ShowWindow(hwnd as HWND, SW_RESTORE);
        }
    }
    take_focus(hwnd)
}

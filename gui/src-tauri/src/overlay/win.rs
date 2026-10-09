//! The overlay's Win32 side, as S0 measured it: the foreground gate, the client area and DPI,
//! and showing the window without activating it, directly above the game in the z-order.

use super::decide::Rect;
use std::os::windows::ffi::OsStringExt;
use tauri::WebviewWindow;
use windows_sys::Win32::Foundation::{CloseHandle, HWND, POINT, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    ClientToScreen, MonitorFromWindow, MONITOR_DEFAULTTONEAREST,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

fn hwnd(window: &WebviewWindow) -> HWND {
    window.hwnd().map_or(std::ptr::null_mut(), |h| h.0 as HWND)
}

pub fn foreground() -> isize {
    unsafe { GetForegroundWindow() as isize }
}

/// The executable of the process that owns `hwnd`, or `None` if it's no longer a window.
fn exe_of(hwnd: isize) -> Option<String> {
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

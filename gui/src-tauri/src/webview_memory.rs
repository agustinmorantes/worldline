//! WebView2's memory target, lowered while a window is hidden.

use tauri::WebviewWindow;

/// Asks WebView2 to use as little memory as it can (`low`) or to go back to normal. The page
/// stays loaded either way. Does nothing on runtimes without `ICoreWebView2_19`, or off Windows.
#[cfg(windows)]
pub fn set_low_memory(window: &WebviewWindow, low: bool) {
    use std::sync::atomic::{AtomicBool, Ordering};
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2_19, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW,
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
    };
    use windows_core::Interface;

    static WARNED: AtomicBool = AtomicBool::new(false);

    let level = if low {
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
    } else {
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
    };
    let _ = window.with_webview(move |wv| {
        let result = unsafe {
            wv.controller()
                .CoreWebView2()
                .and_then(|core| core.cast::<ICoreWebView2_19>())
                .and_then(|core| core.SetMemoryUsageTargetLevel(level))
        };
        if let Err(err) = result {
            if !WARNED.swap(true, Ordering::Relaxed) {
                tauri_plugin_log::log::warn!("can't set the WebView2 memory target: {err}");
            }
        }
    });
}

#[cfg(not(windows))]
pub fn set_low_memory(_window: &WebviewWindow, _low: bool) {}

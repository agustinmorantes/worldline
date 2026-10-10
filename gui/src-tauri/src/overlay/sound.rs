//! The popup's sounds, played from Rust so they work with or without a webview.

use super::decide::Sound;
use tauri_plugin_log::log::warn;
use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};

// Picked by ear on 2026-10-09; the masters are in the design's `sounds/final/`.
static CHECKPOINT: &[u8] = include_bytes!("../../sounds/checkpoint.wav");
static KEPT: &[u8] = include_bytes!("../../sounds/kept.wav");
static NEW_TIMELINE: &[u8] = include_bytes!("../../sounds/new-timeline.wav");
static DEVICE_AHEAD: &[u8] = include_bytes!("../../sounds/device-ahead.wav");
static CAPTURE_FAILED: &[u8] = include_bytes!("../../sounds/capture-failed.wav");

pub fn play(sound: Sound) {
    let wav = match sound {
        Sound::Checkpoint => CHECKPOINT,
        Sound::Kept => KEPT,
        Sound::NewTimeline => NEW_TIMELINE,
        Sound::DeviceAhead => DEVICE_AHEAD,
        Sound::CaptureFailed => CAPTURE_FAILED,
    };
    // `SND_ASYNC` reads the buffer after returning; it's static.
    let flags = SND_MEMORY | SND_ASYNC | SND_NODEFAULT;
    if unsafe { PlaySoundW(wav.as_ptr().cast(), std::ptr::null_mut(), flags) } == 0 {
        warn!("overlay: couldn't play the sound");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The canonical 44-byte header's (channels, sample rate, bits, length in ms).
    fn format(wav: &[u8]) -> (u16, u32, u16, u64) {
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[36..40], b"data");
        let short = |at: usize| u16::from_le_bytes(wav[at..at + 2].try_into().unwrap());
        let long = |at: usize| u32::from_le_bytes(wav[at..at + 4].try_into().unwrap());
        let millis = long(40) as u64 * 1000 / long(28) as u64;
        (short(22), long(24), short(34), millis)
    }

    /// The bundled files: stereo, 44.1 kHz, 16-bit, each 0.75–0.92 s.
    #[test]
    fn the_sounds_are_short_wavs() {
        for (name, wav) in [
            ("checkpoint", CHECKPOINT),
            ("kept", KEPT),
            ("new timeline", NEW_TIMELINE),
            ("device ahead", DEVICE_AHEAD),
            ("capture failed", CAPTURE_FAILED),
        ] {
            let (channels, rate, bits, millis) = format(wav);
            assert_eq!((channels, rate, bits), (2, 44_100, 16), "{name}");
            assert!((750..=920).contains(&millis), "{name}: {millis} ms");
        }
    }
}

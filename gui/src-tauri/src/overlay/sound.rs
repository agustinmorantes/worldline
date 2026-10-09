//! The popup's sounds, played from Rust so they work with or without a webview (handoff, "Sound").

use super::decide::Sound;
use tauri_plugin_log::log::warn;
use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};

/// Provisional: S0's candidate `soft-1-tick`, until the user picks the soft sound by ear.
static SOFT: &[u8] = include_bytes!("../../sounds/soft.wav");

pub fn play(sound: Sound) {
    let wav = match sound {
        Sound::Soft => SOFT,
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

    /// The handoff's limit for *soft*: a tick more than a chime, at most 150 ms.
    #[test]
    fn the_soft_sound_is_a_short_wav() {
        assert_eq!(&SOFT[..4], b"RIFF");
        assert_eq!(&SOFT[8..12], b"WAVE");
        let field = |at: usize| u32::from_le_bytes(SOFT[at..at + 4].try_into().unwrap());
        // The canonical 44-byte header: byte rate at 28, data size at 40.
        assert_eq!(&SOFT[36..40], b"data");
        let millis = field(40) as u64 * 1000 / field(28) as u64;
        assert!(millis <= 150, "{millis} ms");
    }
}

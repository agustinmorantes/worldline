//! Screenshots need Windows 11 (`screenshot.rs`); elsewhere none is taken.

use crate::detection::worker::Shoot;

pub fn supported() -> bool {
    false
}

pub(crate) fn shooter() -> Shoot {
    Box::new(|_| None)
}

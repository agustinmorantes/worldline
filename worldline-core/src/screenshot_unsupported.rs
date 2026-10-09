//! Screenshots need Windows 11 (`screenshot.rs`); elsewhere none is taken.

pub fn supported() -> bool {
    false
}

pub(crate) struct Worker;

impl Worker {
    pub fn start(_done: Box<dyn Fn(i32, Option<Vec<u8>>) + Send>) -> Self {
        Self
    }

    pub fn request(&self, _game_id: i32, _pids: Vec<u32>) -> bool {
        false
    }
}

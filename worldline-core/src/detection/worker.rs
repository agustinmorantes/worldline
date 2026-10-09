//! The thread that takes checkpoint screenshots, one at a time. It exists
//! only while screenshots are on, and waits on its channel when idle.

use std::panic::{self, AssertUnwindSafe};
use std::sync::mpsc;
use std::thread::JoinHandle;

/// Takes a shot of the game running as the given PIDs: the JPEG, or `None`.
/// Lives on the worker's thread.
pub(crate) type Shoot = Box<dyn FnMut(&[u32]) -> Option<Vec<u8>>>;

/// Receives each requested game's JPEG, or `None` when no shot was taken.
pub(crate) type Done = Box<dyn Fn(i32, Option<Vec<u8>>) + Send>;

pub(crate) struct Worker {
    requests: Option<mpsc::SyncSender<(i32, Vec<u32>)>>,
    thread: Option<JoinHandle<()>>,
}

impl Worker {
    /// `shooter` is called once, on the new thread. Every request is
    /// answered through `done`, even when the shot panics.
    pub fn start(shooter: fn() -> Shoot, done: Done) -> Self {
        let (requests, received) = mpsc::sync_channel::<(i32, Vec<u32>)>(1);
        let thread = std::thread::spawn(move || {
            let mut shoot = shooter();
            for (game_id, pids) in received {
                let jpeg = panic::catch_unwind(AssertUnwindSafe(|| shoot(&pids)))
                    .ok()
                    .flatten();
                done(game_id, jpeg);
            }
        });
        Self {
            requests: Some(requests),
            thread: Some(thread),
        }
    }

    /// Hands the worker a shot of the game running as `pids`. `false` when
    /// it is busy: the request is dropped, never queued.
    pub fn request(&self, game_id: i32, pids: Vec<u32>) -> bool {
        self.requests
            .as_ref()
            .is_some_and(|requests| requests.try_send((game_id, pids)).is_ok())
    }

    /// Stops the worker without waiting: a shot in progress still reports,
    /// then the thread ends.
    pub fn detach(mut self) {
        self.requests.take();
        self.thread.take();
    }
}

impl Drop for Worker {
    /// Lets a shot in progress finish, then stops the thread.
    fn drop(&mut self) {
        drop(self.requests.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn panics() -> Shoot {
        Box::new(|pids| {
            assert!(pids.is_empty(), "the shot panics");
            Some(vec![1])
        })
    }

    #[test]
    fn a_panicking_shot_still_reports_and_the_worker_keeps_going() {
        let (tx, rx) = mpsc::channel();
        let worker = Worker::start(
            panics,
            Box::new(move |game_id, jpeg| {
                let _ = tx.send((game_id, jpeg));
            }),
        );
        let answer = || rx.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(worker.request(1, vec![42]));
        assert_eq!(answer(), (1, None));
        assert!(worker.request(2, Vec::new()));
        assert_eq!(answer(), (2, Some(vec![1])));
    }
}

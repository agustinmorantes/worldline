use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

const TIMEOUT: Duration = Duration::from_secs(5);

/// Runs best-effort shutdown once, without blocking Tauri's event loop.
#[derive(Default)]
pub(crate) struct Shutdown {
    started: AtomicBool,
    finished: Arc<AtomicBool>,
}

impl Shutdown {
    pub(crate) fn finished(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }

    pub(crate) fn request(
        &self,
        cleanup: impl FnOnce() + Send + 'static,
        exit: impl FnOnce() + Send + 'static,
    ) {
        self.request_with_timeout(TIMEOUT, cleanup, exit);
    }

    fn request_with_timeout(
        &self,
        timeout: Duration,
        cleanup: impl FnOnce() + Send + 'static,
        exit: impl FnOnce() + Send + 'static,
    ) {
        if self.started.swap(true, Ordering::AcqRel) {
            return;
        }
        let deadline = Instant::now() + timeout;
        let (done, completed) = mpsc::channel();
        let finished = self.finished.clone();
        // The deadline runs independently of captures, screenshots and remote I/O.
        std::thread::spawn(move || {
            if completed.recv_timeout(deadline.saturating_duration_since(Instant::now()))
                == Err(mpsc::RecvTimeoutError::Timeout)
            {
                eprintln!(
                    "Shutdown reached its time limit; remaining work will recover on next launch"
                );
            }
            finished.store(true, Ordering::Release);
            exit();
        });
        std::thread::spawn(move || {
            cleanup();
            let _ = done.send(());
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slow_cleanup_cannot_hold_up_exit_past_the_deadline() {
        let shutdown = Shutdown::default();
        let (exited, exit) = mpsc::channel();
        let (cleaned, cleanup) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let started = Instant::now();
        shutdown.request_with_timeout(
            Duration::from_millis(50),
            move || {
                let _ = wait.recv();
                let _ = cleaned.send(());
            },
            move || {
                let _ = exited.send(());
            },
        );
        assert!(
            started.elapsed() < Duration::from_millis(200),
            "quit blocked the event loop for {:?}",
            started.elapsed()
        );
        exit.recv_timeout(Duration::from_secs(1))
            .expect("exit deadline");
        assert!(shutdown.finished());
        assert!(cleanup.try_recv().is_err(), "exit waited for slow cleanup");
        release.send(()).unwrap();
        cleanup.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(exit.try_recv().is_err(), "late cleanup must not exit again");
    }

    #[test]
    fn completed_cleanup_exits_early_and_repeated_requests_do_nothing() {
        let shutdown = Shutdown::default();
        let (exited, exit) = mpsc::channel();
        let (cleaned, cleanup) = mpsc::channel();
        shutdown.request_with_timeout(
            Duration::from_secs(5),
            move || {
                cleaned.send(()).unwrap();
            },
            move || {
                exited.send(()).unwrap();
            },
        );
        cleanup.recv_timeout(Duration::from_secs(1)).unwrap();
        exit.recv_timeout(Duration::from_secs(1))
            .expect("exit should not wait five seconds");
        assert!(shutdown.finished());
        shutdown.request(|| panic!("duplicate cleanup"), || panic!("duplicate exit"));
    }

    #[test]
    fn the_real_five_second_deadline_exits_even_when_cleanup_is_stuck() {
        let shutdown = Shutdown::default();
        let (release, wait) = mpsc::channel();
        let (exited, exit) = mpsc::channel();
        let started = Instant::now();
        shutdown.request(
            move || {
                let _ = wait.recv();
            },
            move || {
                let _ = exited.send(());
            },
        );
        exit.recv_timeout(Duration::from_secs(6))
            .expect("stuck cleanup exceeded five-second budget");
        let elapsed = started.elapsed();
        assert!(
            elapsed >= Duration::from_secs(5),
            "cleanup did not get its full budget"
        );
        assert!(
            elapsed < Duration::from_secs(6),
            "shutdown took {elapsed:?}"
        );
        assert!(shutdown.finished());
        release.send(()).unwrap();
    }

    #[test]
    fn repeated_requests_while_cleanup_is_running_do_not_start_more_work() {
        let shutdown = Shutdown::default();
        let (release, wait) = mpsc::channel();
        let (exited, exit) = mpsc::channel();
        shutdown.request_with_timeout(
            Duration::from_secs(1),
            move || {
                wait.recv().unwrap();
            },
            move || {
                exited.send(()).unwrap();
            },
        );
        shutdown.request(|| panic!("duplicate cleanup"), || panic!("duplicate exit"));
        assert!(!shutdown.finished());
        release.send(()).unwrap();
        exit.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(shutdown.finished());
    }
}

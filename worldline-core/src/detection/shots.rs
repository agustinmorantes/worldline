//! Decides when a running game is screenshotted: at each real save change,
//! at most once a `FLOOR`, so the shot a checkpoint gets shows the screen
//! when its saves were last written. Pure state plus an injected clock, like
//! `Scheduler`, so every rule is testable without sleeping.
//!
//! Per game, the latest finished shot waits for the checkpoint that contains
//! its change, and a newer change replaces it. A checkpoint can also complete
//! while a shot is in flight; the shot is then settled when it arrives.

use chrono::{Duration, NaiveDateTime};
use std::collections::HashMap;

/// At least this long between two shot attempts of a game.
pub const FLOOR: Duration = Duration::seconds(60);

/// What the watcher knows about a change when it is detected.
#[derive(Debug, Clone, Copy)]
pub struct Change {
    /// The game is in `RunningMonitor`'s running set.
    pub running: bool,
    /// Capture is suppressed (a restore or import is writing the saves).
    pub suppressed: bool,
    /// A real file event: not the startup catch-up, the shutdown flush or a
    /// rescan.
    pub file_event: bool,
}

/// A finished shot to store as a version's screenshot.
#[derive(Debug, PartialEq, Eq)]
pub struct Attach {
    pub version_id: i32,
    pub jpeg: Vec<u8>,
}

pub struct Shots {
    games: HashMap<i32, Game>,
    /// The game whose shot the worker is taking: one in flight, globally.
    in_flight: Option<i32>,
    floor: Duration,
}

impl Default for Shots {
    fn default() -> Self {
        Self::with_floor(FLOOR)
    }
}

#[derive(Default)]
struct Game {
    last_attempt: Option<NaiveDateTime>,
    /// The latest finished shot no checkpoint has settled yet.
    taken: Option<Shot>,
    /// The shot being taken, requested at `last_attempt`, with the outcome
    /// of a checkpoint that completed meanwhile.
    in_flight: Option<Option<Outcome>>,
}

struct Shot {
    /// When its change was reported.
    requested: NaiveDateTime,
    /// The JPEG, or `None` when the shot failed (it still counts).
    jpeg: Option<Vec<u8>>,
}

#[derive(Clone, Copy)]
enum Outcome {
    Created(i32),
    Discarded,
}

impl Outcome {
    fn attach(self, jpeg: Option<Vec<u8>>) -> Option<Attach> {
        match self {
            Self::Created(version_id) => jpeg.map(|jpeg| Attach { version_id, jpeg }),
            Self::Discarded => None,
        }
    }
}

impl Shots {
    /// `floor`: at least this long between two shot attempts of a game.
    pub fn with_floor(floor: Duration) -> Self {
        Self {
            games: HashMap::new(),
            in_flight: None,
            floor,
        }
    }

    /// A change of `game` was detected. Asks for a shot through `request`
    /// (the worker's `try_send`) when every rule allows one; returns whether
    /// the worker took it. A refused request is dropped, never queued.
    pub fn changed(
        &mut self,
        game: i32,
        change: Change,
        now: NaiveDateTime,
        request: impl FnOnce() -> bool,
    ) -> bool {
        if !change.running || change.suppressed || !change.file_event || self.in_flight.is_some() {
            return false;
        }
        let state = self.games.entry(game).or_default();
        let floor_passed = state.last_attempt.is_none_or(|at| now - at >= self.floor);
        if state.in_flight.is_some() || !floor_passed || !request() {
            return false;
        }
        state.in_flight = Some(None);
        state.last_attempt = Some(now);
        self.in_flight = Some(game);
        true
    }

    /// The worker finished `game`'s shot (`None`: it failed). Returns the
    /// shot to attach when its checkpoint was created meanwhile; otherwise it
    /// replaces the game's earlier shot, which shows an older screen.
    pub fn finished(&mut self, game: i32, jpeg: Option<Vec<u8>>) -> Option<Attach> {
        if self.in_flight == Some(game) {
            self.in_flight = None;
        }
        // A forgotten game's shot is dropped.
        let state = self.games.get_mut(&game)?;
        match state.in_flight.take()? {
            Some(outcome) => outcome.attach(jpeg),
            None => {
                state.taken = state.last_attempt.map(|requested| Shot { requested, jpeg });
                None
            }
        }
    }

    /// A checkpoint of `game` that `started` (before reading the saves)
    /// completed: `created` is the new version, `None` when nothing changed.
    /// Returns the shot to attach: the latest one whose change it contains.
    ///
    /// A checkpoint that started before a shot's change was written (the
    /// change was reported `debounce` after the write) can't contain it, and
    /// leaves that shot for the next one.
    pub fn checkpointed(
        &mut self,
        game: i32,
        created: Option<i32>,
        started: NaiveDateTime,
        debounce: Duration,
    ) -> Option<Attach> {
        let state = self.games.get_mut(&game)?;
        let contains = |requested: NaiveDateTime| started >= requested - debounce;
        let outcome = created.map_or(Outcome::Discarded, Outcome::Created);
        if let Some(first) = &mut state.in_flight
            && state.last_attempt.is_some_and(contains)
        {
            // The first outcome settles the shot; later ones belong to the
            // next. The earlier shot shows an older screen.
            first.get_or_insert(outcome);
            state.taken = None;
            return None;
        }
        let shot = state.taken.take_if(|shot| contains(shot.requested))?;
        outcome.attach(shot.jpeg)
    }

    /// The worker was stopped: a new one may take a request at once. The
    /// stopped one still reports its shot in progress, through `finished`.
    pub fn worker_stopped(&mut self) {
        self.in_flight = None;
    }

    /// The game is gone: its shot is dropped.
    pub fn forget(&mut self, game: i32) {
        self.games.remove(&game);
    }

    /// The games with any state, to forget those that are gone.
    pub fn games(&self) -> Vec<i32> {
        self.games.keys().copied().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(seconds: i64) -> NaiveDateTime {
        chrono::DateTime::from_timestamp(1_800_000_000 + seconds, 0)
            .unwrap()
            .naive_utc()
    }

    const PLAYING: Change = Change {
        running: true,
        suppressed: false,
        file_event: true,
    };

    fn jpeg(n: u8) -> Option<Vec<u8>> {
        Some(vec![n])
    }

    /// A checkpoint started after every change in the test.
    fn late() -> NaiveDateTime {
        at(1_000_000)
    }

    #[test]
    fn a_burst_of_changes_shoots_at_most_once_a_floor() {
        let mut shots = Shots::default();
        let mut requests = 0;
        for second in (0..300).step_by(10) {
            if shots.changed(1, PLAYING, at(second), || {
                requests += 1;
                true
            }) {
                assert_eq!(shots.finished(1, jpeg(1)), None);
            }
        }
        assert_eq!(requests, 5);
    }

    /// Silksong, with a 15-minute cooldown: saved at the bone bottom, then
    /// before the miniboss five minutes later; the player keeps the second.
    #[test]
    fn a_later_change_replaces_the_held_shot() {
        let debounce = Duration::seconds(5);
        let mut shots = Shots::default();
        assert!(shots.changed(1, PLAYING, at(0), || true));
        assert_eq!(shots.finished(1, jpeg(1)), None);
        // Inside the floor: the shot of the first change stays.
        assert!(!shots.changed(1, PLAYING, at(30), || panic!("floor")));
        assert!(shots.changed(1, PLAYING, at(300), || true));
        assert_eq!(shots.finished(1, jpeg(2)), None);
        // The manual checkpoint, 44 seconds after the second save.
        assert_eq!(
            shots.checkpointed(1, Some(19), at(344), debounce),
            Some(Attach {
                version_id: 19,
                jpeg: vec![2]
            })
        );
        // Nothing left for the next one.
        assert_eq!(shots.checkpointed(1, Some(20), at(400), debounce), None);
    }

    #[test]
    fn a_checkpoint_meets_the_shot_of_the_latest_change_it_contains() {
        let debounce = Duration::seconds(5);
        let mut shots = Shots::default();
        assert!(shots.changed(1, PLAYING, at(0), || true));
        shots.finished(1, jpeg(1));
        // A newer change (written at ~95) is being shot when a checkpoint that
        // read the saves at 90 completes: it gets the earlier shot.
        assert!(shots.changed(1, PLAYING, at(100), || true));
        assert_eq!(
            shots.checkpointed(1, Some(10), at(90), debounce),
            Some(Attach {
                version_id: 10,
                jpeg: vec![1]
            })
        );
        assert_eq!(shots.finished(1, jpeg(2)), None);
        assert_eq!(
            shots.checkpointed(1, Some(11), at(120), debounce),
            Some(Attach {
                version_id: 11,
                jpeg: vec![2]
            })
        );

        // One that contains the newer change waits for its shot, and the
        // earlier one never reaches a later version.
        assert!(shots.changed(1, PLAYING, at(200), || true));
        shots.finished(1, jpeg(3));
        assert!(shots.changed(1, PLAYING, at(300), || true));
        assert_eq!(shots.checkpointed(1, None, at(310), debounce), None);
        assert_eq!(shots.finished(1, jpeg(4)), None, "unchanged: dropped");
        assert_eq!(shots.checkpointed(1, Some(12), at(320), debounce), None);
    }

    #[test]
    fn a_busy_worker_drops_the_request_and_nothing_catches_up() {
        let mut shots = Shots::default();
        assert!(shots.changed(1, PLAYING, at(0), || true));
        // Another game's change while game 1 is in flight never reaches the worker.
        assert!(!shots.changed(2, PLAYING, at(1), || panic!("busy")));
        assert_eq!(shots.finished(1, jpeg(1)), None);
        // Nothing is replayed once the worker is free...
        let mut requests = 0;
        shots.changed(1, PLAYING, at(2), || {
            requests += 1;
            true
        });
        assert_eq!(requests, 0, "game 1 is inside its floor");
        // ...and a refused `try_send` is dropped too: the game stays armed
        // for its next real change, nothing is queued.
        assert!(!shots.changed(2, PLAYING, at(3), || false));
        assert!(shots.changed(2, PLAYING, at(4), || true));
    }

    #[test]
    fn the_floor_holds_across_a_checkpoint() {
        let mut shots = Shots::default();
        assert!(shots.changed(1, PLAYING, at(0), || true));
        shots.finished(1, jpeg(1));
        shots.checkpointed(1, Some(10), late(), Duration::zero());
        assert!(!shots.changed(1, PLAYING, at(59), || true));
        assert!(shots.changed(1, PLAYING, at(60), || true));
    }

    #[test]
    fn no_shot_unless_running_unsuppressed_and_from_a_real_file_event() {
        let mut shots = Shots::default();
        for change in [
            Change {
                running: false,
                ..PLAYING
            },
            Change {
                suppressed: true,
                ..PLAYING
            },
            // Startup catch-up, shutdown flush and rescans aren't file events.
            Change {
                file_event: false,
                ..PLAYING
            },
        ] {
            assert!(!shots.changed(1, change, at(0), || panic!("no shot")));
        }
        assert!(shots.changed(1, PLAYING, at(0), || true), "still armed");
    }

    #[test]
    fn an_unchanged_or_empty_checkpoint_discards_the_shot() {
        let mut shots = Shots::default();
        assert!(shots.changed(1, PLAYING, at(0), || true));
        shots.finished(1, jpeg(1));
        assert_eq!(shots.checkpointed(1, None, late(), Duration::zero()), None);
        // Re-armed, and the old shot is gone.
        assert!(shots.changed(1, PLAYING, at(60), || true));
        shots.finished(1, jpeg(2));
        assert_eq!(
            shots.checkpointed(1, Some(11), late(), Duration::zero()),
            Some(Attach {
                version_id: 11,
                jpeg: vec![2]
            })
        );
    }

    #[test]
    fn a_shot_finishing_after_its_checkpoint_still_attaches() {
        let mut shots = Shots::default();
        assert!(shots.changed(1, PLAYING, at(0), || true));
        assert_eq!(
            shots.checkpointed(1, Some(10), late(), Duration::zero()),
            None
        );
        // A later checkpoint belongs to the next shot.
        assert_eq!(
            shots.checkpointed(1, Some(11), late(), Duration::zero()),
            None
        );
        assert_eq!(
            shots.finished(1, jpeg(1)),
            Some(Attach {
                version_id: 10,
                jpeg: vec![1]
            })
        );
        assert!(shots.changed(1, PLAYING, at(60), || true), "re-armed");

        // Discarded before it finished: dropped on arrival.
        assert_eq!(shots.checkpointed(1, None, late(), Duration::zero()), None);
        assert_eq!(shots.finished(1, jpeg(2)), None);
        assert_eq!(
            shots.checkpointed(1, Some(12), late(), Duration::zero()),
            None
        );
    }

    #[test]
    fn a_failed_shot_counts_and_attaches_nothing() {
        let mut shots = Shots::default();
        assert!(shots.changed(1, PLAYING, at(0), || true));
        assert_eq!(shots.finished(1, None), None);
        assert!(!shots.changed(1, PLAYING, at(30), || true), "the floor");
        assert_eq!(
            shots.checkpointed(1, Some(10), late(), Duration::zero()),
            None
        );
        // A failed newer shot also drops the earlier one's older screen.
        assert!(shots.changed(1, PLAYING, at(100), || true));
        shots.finished(1, jpeg(1));
        assert!(shots.changed(1, PLAYING, at(200), || true));
        assert_eq!(shots.finished(1, None), None);
        assert_eq!(
            shots.checkpointed(1, Some(11), late(), Duration::zero()),
            None
        );
    }

    #[test]
    fn a_deleted_game_drops_its_shot() {
        let mut shots = Shots::default();
        assert!(shots.changed(1, PLAYING, at(0), || true));
        shots.forget(1);
        assert_eq!(shots.finished(1, jpeg(1)), None);
        assert!(shots.games().is_empty());
        // The worker is free again.
        assert!(shots.changed(2, PLAYING, at(1), || true));
    }

    #[test]
    fn a_checkpoint_that_started_before_the_change_was_written_leaves_the_shot_pending() {
        let debounce = Duration::seconds(2);
        let mut shots = Shots::default();
        // Save Y written at ~98, reported at 100: the shot shows Y.
        assert!(shots.changed(1, PLAYING, at(100), || true));
        // A checkpoint that read the saves at 90 (before Y) completes now.
        assert_eq!(shots.checkpointed(1, Some(10), at(90), debounce), None);
        assert_eq!(shots.finished(1, jpeg(1)), None, "still pending");
        assert_eq!(shots.checkpointed(1, None, at(95), debounce), None);
        // The checkpoint of Y gets it.
        assert_eq!(
            shots.checkpointed(1, Some(11), at(98), debounce),
            Some(Attach {
                version_id: 11,
                jpeg: vec![1]
            })
        );

        // Same while the shot is still in flight.
        assert!(shots.changed(1, PLAYING, at(200), || true));
        assert_eq!(shots.checkpointed(1, Some(12), at(150), debounce), None);
        assert_eq!(shots.checkpointed(1, Some(13), at(201), debounce), None);
        assert_eq!(
            shots.finished(1, jpeg(2)),
            Some(Attach {
                version_id: 13,
                jpeg: vec![2]
            })
        );
    }

    #[test]
    fn a_stopped_worker_frees_the_slot_and_its_late_shot_still_settles() {
        let mut shots = Shots::default();
        assert!(shots.changed(1, PLAYING, at(0), || true));
        shots.worker_stopped();
        assert!(shots.changed(2, PLAYING, at(1), || true), "not stuck");
        assert!(
            !shots.changed(1, PLAYING, at(100), || true),
            "1 awaits its shot"
        );
        assert_eq!(shots.finished(1, jpeg(1)), None);
        assert_eq!(
            shots.checkpointed(1, Some(10), late(), Duration::zero()),
            Some(Attach {
                version_id: 10,
                jpeg: vec![1]
            })
        );
        // Game 2's own result still frees the slot.
        assert_eq!(shots.finished(2, None), None);
        assert!(shots.changed(1, PLAYING, at(200), || true));
    }
}

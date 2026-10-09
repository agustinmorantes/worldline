//! Decides when a running game is screenshotted: one shot per pending
//! checkpoint, at the first real save change after the last one. Pure state
//! plus an injected clock, like `Scheduler`, so every rule is testable
//! without sleeping.
//!
//! Per game: Armed → in flight → taken → (checkpoint completes) → Armed. A
//! checkpoint can also complete while the shot is in flight; the shot is then
//! settled when it arrives.

use chrono::{Duration, NaiveDateTime};
use std::collections::HashMap;

/// At least this long between two shot attempts of a game, even if it is
/// re-armed sooner.
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

#[derive(Default)]
pub struct Shots {
    games: HashMap<i32, Game>,
    /// The game whose shot the worker is taking: one in flight, globally.
    in_flight: Option<i32>,
}

#[derive(Default)]
struct Game {
    phase: Phase,
    last_attempt: Option<NaiveDateTime>,
}

#[derive(Default)]
enum Phase {
    #[default]
    Armed,
    /// With the outcome of a checkpoint that completed meanwhile.
    InFlight(Option<Outcome>),
    /// The JPEG, or `None` when the shot failed (it still counts).
    Taken(Option<Vec<u8>>),
}

#[derive(Clone, Copy)]
enum Outcome {
    Created(i32),
    Discarded,
}

impl Shots {
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
        let armed = matches!(state.phase, Phase::Armed);
        let floor_passed = state.last_attempt.is_none_or(|at| now - at >= FLOOR);
        if !armed || !floor_passed || !request() {
            return false;
        }
        state.phase = Phase::InFlight(None);
        state.last_attempt = Some(now);
        self.in_flight = Some(game);
        true
    }

    /// The worker finished `game`'s shot (`None`: it failed). Returns the
    /// shot to attach when its checkpoint was created meanwhile.
    pub fn finished(&mut self, game: i32, jpeg: Option<Vec<u8>>) -> Option<Attach> {
        if self.in_flight == Some(game) {
            self.in_flight = None;
        }
        // A forgotten game's shot is dropped.
        let state = self.games.get_mut(&game)?;
        let Phase::InFlight(outcome) = state.phase else {
            return None;
        };
        match outcome {
            None => {
                state.phase = Phase::Taken(jpeg);
                None
            }
            Some(outcome) => {
                state.phase = Phase::Armed;
                match outcome {
                    Outcome::Created(version_id) => jpeg.map(|jpeg| Attach { version_id, jpeg }),
                    Outcome::Discarded => None,
                }
            }
        }
    }

    /// A checkpoint of `game` completed: `created` is the new version, `None`
    /// when nothing changed. Re-arms the game and returns the shot to attach.
    pub fn checkpointed(&mut self, game: i32, created: Option<i32>) -> Option<Attach> {
        let state = self.games.get_mut(&game)?;
        let outcome = created.map_or(Outcome::Discarded, Outcome::Created);
        match std::mem::take(&mut state.phase) {
            Phase::Armed => None,
            // The first outcome settles the shot; later ones belong to the next.
            Phase::InFlight(first) => {
                state.phase = Phase::InFlight(first.or(Some(outcome)));
                None
            }
            Phase::Taken(jpeg) => match outcome {
                Outcome::Created(version_id) => jpeg.map(|jpeg| Attach { version_id, jpeg }),
                Outcome::Discarded => None,
            },
        }
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

    #[test]
    fn a_burst_inside_the_cooldown_takes_one_shot() {
        let mut shots = Shots::default();
        let mut requests = 0;
        for second in 0..20 {
            shots.changed(1, PLAYING, at(second * 45), || {
                requests += 1;
                true
            });
            if second == 3 {
                assert_eq!(shots.finished(1, jpeg(1)), None);
            }
        }
        assert_eq!(requests, 1);
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
        assert_eq!(requests, 0, "game 1 already has its shot");
        // ...and a refused `try_send` is dropped too: the game stays armed
        // for its next real change, nothing is queued.
        assert!(!shots.changed(2, PLAYING, at(3), || false));
        assert!(shots.changed(2, PLAYING, at(4), || true));
    }

    #[test]
    fn a_completed_checkpoint_rearms_and_the_next_change_shoots_again() {
        let mut shots = Shots::default();
        assert!(shots.changed(1, PLAYING, at(0), || true));
        assert_eq!(shots.finished(1, jpeg(1)), None);
        assert!(!shots.changed(1, PLAYING, at(100), || true));
        assert_eq!(
            shots.checkpointed(1, Some(10)),
            Some(Attach {
                version_id: 10,
                jpeg: vec![1]
            })
        );
        assert!(shots.changed(1, PLAYING, at(200), || true));
    }

    #[test]
    fn the_floor_holds_even_when_rearmed_early() {
        let mut shots = Shots::default();
        assert!(shots.changed(1, PLAYING, at(0), || true));
        shots.finished(1, jpeg(1));
        shots.checkpointed(1, Some(10));
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
        assert_eq!(shots.checkpointed(1, None), None);
        // Re-armed, and the old shot is gone.
        assert!(shots.changed(1, PLAYING, at(60), || true));
        shots.finished(1, jpeg(2));
        assert_eq!(
            shots.checkpointed(1, Some(11)),
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
        assert_eq!(shots.checkpointed(1, Some(10)), None);
        // A later checkpoint belongs to the next shot.
        assert_eq!(shots.checkpointed(1, Some(11)), None);
        assert_eq!(
            shots.finished(1, jpeg(1)),
            Some(Attach {
                version_id: 10,
                jpeg: vec![1]
            })
        );
        assert!(shots.changed(1, PLAYING, at(60), || true), "re-armed");

        // Discarded before it finished: dropped on arrival.
        assert_eq!(shots.checkpointed(1, None), None);
        assert_eq!(shots.finished(1, jpeg(2)), None);
        assert_eq!(shots.checkpointed(1, Some(12)), None);
    }

    #[test]
    fn a_failed_shot_counts_and_attaches_nothing() {
        let mut shots = Shots::default();
        assert!(shots.changed(1, PLAYING, at(0), || true));
        assert_eq!(shots.finished(1, None), None);
        assert!(!shots.changed(1, PLAYING, at(100), || true), "no retry");
        assert_eq!(shots.checkpointed(1, Some(10)), None);
        assert!(shots.changed(1, PLAYING, at(200), || true));
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
}

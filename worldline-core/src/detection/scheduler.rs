//! Decides when changed games are captured. Pure state plus an injected
//! clock, so every rule is testable without sleeping.

use chrono::{Duration, NaiveDateTime};
use std::collections::BTreeMap;

const MAX_BACKOFF_SECONDS: i64 = 10 * 60;

#[derive(Default)]
pub struct Scheduler {
    /// Changed games awaiting capture, with the earliest retry after a failure.
    pending: BTreeMap<i32, Pending>,
}

#[derive(Clone, Copy, Default)]
struct Pending {
    retry_at: Option<NaiveDateTime>,
    failures: u32,
}

impl Scheduler {
    pub fn changed(&mut self, game: i32) {
        self.pending.entry(game).or_default();
    }

    pub fn is_pending(&self, game: i32) -> bool {
        self.pending.contains_key(&game)
    }

    /// The games awaiting capture.
    pub fn pending(&self) -> Vec<i32> {
        self.pending.keys().copied().collect()
    }

    /// When a failed game is retried, if it failed.
    pub fn retry_at(&self, game: i32) -> Option<NaiveDateTime> {
        self.pending.get(&game).and_then(|pending| pending.retry_at)
    }

    /// Games to capture now. `eligible_at` gives the end of a game's cooldown,
    /// or `None` when the game no longer exists (it is then dropped).
    pub fn due(
        &mut self,
        now: NaiveDateTime,
        eligible_at: impl Fn(i32) -> Option<Option<NaiveDateTime>>,
    ) -> Vec<i32> {
        let mut due = Vec::new();
        self.pending
            .retain(|&game, pending| match eligible_at(game) {
                None => false,
                Some(eligible) => {
                    let ready = eligible.is_none_or(|at| at <= now)
                        && pending.retry_at.is_none_or(|at| at <= now);
                    if ready {
                        due.push(game);
                    }
                    true
                }
            });
        due
    }

    pub fn completed(&mut self, game: i32) {
        self.pending.remove(&game);
    }

    /// Keeps the game pending and backs off exponentially, per game.
    pub fn failed(&mut self, game: i32, now: NaiveDateTime) {
        let pending = self.pending.entry(game).or_default();
        pending.failures += 1;
        let delay = 2_i64
            .saturating_pow(pending.failures.min(20))
            .min(MAX_BACKOFF_SECONDS);
        pending.retry_at = Some(now + Duration::seconds(delay));
    }

    /// How long until the next pending game may become due.
    pub fn next_wakeup(
        &self,
        now: NaiveDateTime,
        eligible_at: impl Fn(i32) -> Option<Option<NaiveDateTime>>,
    ) -> Option<std::time::Duration> {
        self.pending
            .iter()
            .map(|(&game, pending)| {
                let eligible = eligible_at(game).flatten().unwrap_or(now);
                pending
                    .retry_at
                    .map_or(eligible, |retry| retry.max(eligible))
            })
            .min()
            .map(|at| (at - now).to_std().unwrap_or_default())
    }

    /// Everything still pending, for a flush on shutdown.
    pub fn drain(&mut self) -> Vec<i32> {
        std::mem::take(&mut self.pending).into_keys().collect()
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

    #[test]
    fn cooldown_defers_until_eligible_and_coalesces_changes() {
        let mut scheduler = Scheduler::default();
        let eligible = |_| Some(Some(at(60)));
        scheduler.changed(1);
        scheduler.changed(1);
        assert!(scheduler.due(at(0), eligible).is_empty());
        assert_eq!(
            scheduler.next_wakeup(at(0), eligible),
            Some(std::time::Duration::from_secs(60))
        );
        assert_eq!(scheduler.due(at(60), eligible), vec![1]);
        scheduler.completed(1);
        assert_eq!(scheduler.next_wakeup(at(61), eligible), None);
    }

    #[test]
    fn failures_back_off_per_game_without_blocking_others() {
        let mut scheduler = Scheduler::default();
        let eligible = |_| Some(None);
        scheduler.changed(1);
        scheduler.changed(2);
        scheduler.failed(1, at(0));
        assert_eq!(scheduler.due(at(0), eligible), vec![2]);
        assert_eq!(scheduler.due(at(2), eligible), vec![1, 2]);
        scheduler.failed(1, at(2));
        assert_eq!(scheduler.due(at(5), eligible), vec![2]);
        assert_eq!(scheduler.due(at(6), eligible), vec![1, 2]);
    }

    #[test]
    fn deleted_games_are_dropped_and_drain_flushes_the_rest() {
        let mut scheduler = Scheduler::default();
        scheduler.changed(1);
        scheduler.changed(2);
        assert_eq!(
            scheduler.due(at(0), |game| (game == 2).then_some(Some(at(99)))),
            Vec::<i32>::new()
        );
        assert!(!scheduler.is_pending(1));
        assert_eq!(scheduler.drain(), vec![2]);
        assert_eq!(scheduler.next_wakeup(at(0), |_| Some(None)), None);
    }
}

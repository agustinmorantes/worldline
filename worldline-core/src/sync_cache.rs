//! What this device last read from a sync remote, so a sync reads only the
//! games whose stamps changed (ADR 0012). Derived and device-local: when it
//! is missing, unreadable, for another remote or due a check, the next sync
//! reads every game again.

use crate::store::{GameRecord, RetirementMarker, Store, read_json, write_json};
use crate::sync::{GameStamp, Remote};
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use uuid::Uuid;

const FORMAT: u32 = 1;
const INDEX: &str = "remote.json";

/// How long syncs may trust stamps before every game is read and pushed
/// again, catching what no stamp signals.
fn verify_every() -> chrono::Duration {
    chrono::Duration::days(1)
}

#[derive(Serialize, Deserialize)]
struct Index {
    format: u32,
    root: PathBuf,
    store_created_at: DateTime<Utc>,
    verified_at: Option<DateTime<Utc>>,
    markers: HashMap<Uuid, RetirementMarker>,
    games: HashMap<Uuid, Entry>,
    /// Whether the pulls recorded in `games` brought screenshots.
    #[serde(default)]
    screenshots: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Entry {
    /// Each device's stamp digest when the record was read.
    seen: BTreeMap<Uuid, String>,
    /// The record holds everything the stamps in `seen` describe.
    settled: bool,
    /// Pulled here with nothing missing.
    pulled: bool,
    /// This device's stamp digest after its last verified push.
    pushed: Option<String>,
}

/// What a refresh read.
#[derive(Debug, Default)]
pub(crate) struct Refresh {
    /// Every game was read.
    pub full: bool,
    /// Games read this time.
    pub read: HashSet<Uuid>,
    /// Games with markers new to the cache.
    pub marked: HashSet<Uuid>,
}

pub(crate) struct RemoteCache {
    dir: PathBuf,
    root: PathBuf,
    store_created_at: DateTime<Utc>,
    verified_at: Option<DateTime<Utc>>,
    screenshots: bool,
    /// The remote's retirement markers.
    pub markers: HashMap<Uuid, RetirementMarker>,
    entries: HashMap<Uuid, Entry>,
    records: HashMap<Uuid, GameRecord>,
    /// Records to write, and to delete, on save.
    written: HashSet<Uuid>,
    deleted: HashSet<Uuid>,
    /// Something to save: an idle sync writes nothing.
    dirty: bool,
}

/// Each device's stamp per game.
fn stamps_by_game(remote: &Remote) -> HashMap<Uuid, BTreeMap<Uuid, &GameStamp>> {
    let mut games: HashMap<Uuid, BTreeMap<Uuid, &GameStamp>> = HashMap::new();
    for state in &remote.states {
        for (game, stamp) in state.stamps.iter().flatten() {
            games
                .entry(*game)
                .or_default()
                .insert(state.device_uuid, stamp);
        }
    }
    games
}

fn root_of(store: &Store) -> PathBuf {
    std::fs::canonicalize(store.root()).unwrap_or_else(|_| store.root().to_path_buf())
}

impl RemoteCache {
    /// The cache kept in `dir` when it is for `remote`; otherwise an empty one.
    pub fn load(dir: &Path, remote: &Remote) -> Result<Self> {
        let mut cache = Self {
            dir: dir.to_path_buf(),
            root: root_of(&remote.store),
            store_created_at: remote.store.created_at()?,
            verified_at: None,
            screenshots: false,
            markers: HashMap::new(),
            entries: HashMap::new(),
            records: HashMap::new(),
            written: HashSet::new(),
            deleted: HashSet::new(),
            dirty: true,
        };
        let Ok(index) = read_json::<Index>(&dir.join(INDEX)) else {
            return Ok(cache);
        };
        if index.format != FORMAT
            || index.root != cache.root
            || index.store_created_at != cache.store_created_at
        {
            return Ok(cache);
        }
        for (uuid, entry) in index.games {
            match read_json::<GameRecord>(&cache.record_path(uuid)) {
                Ok(record) if record.meta.uuid == uuid => {
                    cache.records.insert(uuid, record);
                    cache.entries.insert(uuid, entry);
                }
                // A record lost after its index entry is read again.
                _ => {}
            }
        }
        cache.verified_at = index.verified_at;
        cache.screenshots = index.screenshots;
        cache.markers = index.markers;
        cache.dirty = false;
        Ok(cache)
    }

    /// Whether this is the cache of `remote`: same folder, same store.
    pub fn is_for(&self, remote: &Remote) -> bool {
        root_of(&remote.store) == self.root
            && remote
                .store
                .created_at()
                .is_ok_and(|at| at == self.store_created_at)
    }

    fn record_path(&self, uuid: Uuid) -> PathBuf {
        self.dir.join("games").join(format!("{uuid}.json"))
    }

    /// Reads the games whose stamps changed, the ones not settled yet and
    /// new folders. Reads every game when a check is due, or when some
    /// device's stamps are unknown (an older client, an unreadable state).
    pub fn refresh(&mut self, remote: &Remote, now: DateTime<Utc>) -> Result<Refresh> {
        let verify = self.verified_at.is_none_or(|at| now - at >= verify_every());
        if verify {
            // Forgets pushes too, so every game is pushed again.
            self.deleted.extend(self.entries.keys());
            self.entries.clear();
            self.records.clear();
            self.markers.clear();
            self.written.clear();
            self.verified_at = Some(now);
            self.dirty = true;
        }
        let full = verify
            || remote.unreadable_state.is_some()
            || remote.states.iter().any(|state| state.stamps.is_none());
        let mut refresh = Refresh {
            full,
            marked: self.refresh_markers(&remote.store)?,
            ..Refresh::default()
        };
        let slugs: HashSet<String> = remote.store.game_slugs()?.into_iter().collect();
        let by_slug: HashMap<String, Uuid> = self
            .records
            .iter()
            .map(|(uuid, record)| (record.meta.folder_slug.clone(), *uuid))
            .collect();
        for (slug, uuid) in &by_slug {
            if !slugs.contains(slug) {
                self.forget(*uuid);
            }
        }
        let stamps = stamps_by_game(remote);
        let no_stamps = BTreeMap::new();
        for slug in slugs {
            let known = by_slug.get(&slug).copied();
            let current = |uuid| stamps.get(&uuid).unwrap_or(&no_stamps);
            let unchanged = known.is_some_and(|uuid| {
                self.entries.get(&uuid).is_some_and(|entry| {
                    entry.settled
                        && entry
                            .seen
                            .iter()
                            .map(|(device, digest)| (device, digest.as_str()))
                            .eq(current(uuid)
                                .iter()
                                .map(|(device, stamp)| (device, stamp.digest.as_str())))
                })
            });
            if unchanged && !full {
                continue;
            }
            let Some(record) = remote.store.game_by_slug(&slug)? else {
                // Not arrived, or no longer readable: absent, not an error.
                if let Some(uuid) = known {
                    self.forget(uuid);
                }
                continue;
            };
            let uuid = record.meta.uuid;
            let stamps = current(uuid);
            let entry = self.entries.entry(uuid).or_default();
            entry.seen = stamps
                .iter()
                .map(|(device, stamp)| (*device, stamp.digest.clone()))
                .collect();
            // Anything a stamp describes that hasn't arrived is read again
            // next time, whether or not its writer changes anything.
            entry.settled = stamps.values().all(|stamp| stamp.arrived_in(&record));
            self.records.insert(uuid, record);
            self.written.insert(uuid);
            self.deleted.remove(&uuid);
            self.dirty = true;
            refresh.read.insert(uuid);
        }
        Ok(refresh)
    }

    /// Reads the markers not seen yet (they are write-once) and returns
    /// their games, which are pulled again: whichever call reads a marker
    /// first, the next sync applies it.
    pub fn refresh_markers(&mut self, store: &Store) -> Result<HashSet<Uuid>> {
        let listed = store.marker_uuids()?;
        let before = self.markers.len();
        self.markers.retain(|uuid, _| listed.contains(uuid));
        self.dirty |= self.markers.len() != before;
        let mut games = HashSet::new();
        for uuid in listed {
            if self.markers.contains_key(&uuid) {
                continue;
            }
            // A marker that can't be read yet is tried again next time.
            if let Some(marker) = store.marker(uuid) {
                games.insert(marker.game_uuid);
                if let Some(entry) = self.entries.get_mut(&marker.game_uuid) {
                    entry.pulled = false;
                }
                self.markers.insert(uuid, marker);
                self.dirty = true;
            }
        }
        Ok(games)
    }

    fn forget(&mut self, uuid: Uuid) {
        self.dirty = true;
        self.entries.remove(&uuid);
        self.records.remove(&uuid);
        self.written.remove(&uuid);
        self.deleted.insert(uuid);
    }

    /// The remote's games as last read, in folder order.
    pub fn catalog(&self) -> Vec<GameRecord> {
        let mut games: Vec<GameRecord> = self.records.values().cloned().collect();
        games.sort_by(|a, b| a.meta.folder_slug.cmp(&b.meta.folder_slug));
        games
    }

    pub fn pulled(&self, uuid: Uuid) -> bool {
        self.entries.get(&uuid).is_some_and(|entry| entry.pulled)
    }

    /// Whether syncs pull screenshots: turning it on pulls every game again,
    /// for the screenshots left behind while it was off.
    pub fn sync_screenshots(&mut self, on: bool) {
        if on == self.screenshots {
            return;
        }
        if on {
            for entry in self.entries.values_mut() {
                entry.pulled = false;
            }
        }
        self.screenshots = on;
        self.dirty = true;
    }

    /// Records whether the game was pulled with nothing missing; when not,
    /// the next sync pulls it again.
    pub fn set_pulled(&mut self, uuid: Uuid, pulled: bool) {
        if let Some(entry) = self.entries.get_mut(&uuid)
            && entry.pulled != pulled
        {
            entry.pulled = pulled;
            self.dirty = true;
        }
    }

    /// The versions the remote holds of a game this device already pushed as
    /// `stamp` describes it, when the remote still holds all of it.
    pub fn unchanged(&self, stamp: &GameStamp, uuid: Uuid) -> Option<HashSet<Uuid>> {
        let record = self.records.get(&uuid)?;
        (self.entries.get(&uuid)?.pushed.as_deref() == Some(stamp.digest.as_str())
            && stamp.arrived_in(record))
        .then(|| record.versions().map(|version| version.uuid).collect())
    }

    /// A verified push by `device` of `stamp`, with `record` read afterwards.
    /// Other devices' stamps stay as last seen, so a change they published
    /// meanwhile is still read and pulled.
    pub fn pushed(&mut self, device: Uuid, record: GameRecord, stamp: &GameStamp) {
        let uuid = record.meta.uuid;
        let new = !self.entries.contains_key(&uuid);
        let entry = self.entries.entry(uuid).or_default();
        entry.pushed = Some(stamp.digest.clone());
        // A game first read here: any other device's stamp differs from
        // `seen` and has it read next time.
        if new {
            entry.settled = stamp.arrived_in(&record);
        }
        if entry.seen.contains_key(&device) || entry.settled {
            entry.seen.insert(device, stamp.digest.clone());
        }
        self.records.insert(uuid, record);
        self.written.insert(uuid);
        self.deleted.remove(&uuid);
        self.dirty = true;
    }

    /// A push that left something out: the next one pushes the game again.
    pub fn not_pushed(&mut self, uuid: Uuid) {
        if let Some(entry) = self.entries.get_mut(&uuid)
            && entry.pushed.take().is_some()
        {
            self.dirty = true;
        }
    }

    /// Writes what changed. Records go first: an index that is older than
    /// them only makes the next sync read more.
    pub fn save(&mut self) -> Result<()> {
        if !self.dirty {
            return Ok(());
        }
        for uuid in &self.written {
            write_json(&self.record_path(*uuid), &self.records[uuid])?;
        }
        write_json(
            &self.dir.join(INDEX),
            &Index {
                format: FORMAT,
                root: self.root.clone(),
                store_created_at: self.store_created_at,
                verified_at: self.verified_at,
                screenshots: self.screenshots,
                markers: self.markers.clone(),
                games: self.entries.clone(),
            },
        )?;
        for uuid in &self.deleted {
            let _ = std::fs::remove_file(self.record_path(*uuid));
        }
        self.written.clear();
        self.deleted.clear();
        self.dirty = false;
        Ok(())
    }
}

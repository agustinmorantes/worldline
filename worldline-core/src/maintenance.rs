//! Retention (which versions to retire) and garbage collection (which chunks
//! to delete), identical for the local store and the remote. See
//! `docs/pruning-plan.md` and ADR 0002.

use crate::store::{GameRecord, RetirementMarker, Store, read_json, write_json};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Datelike, Duration, Timelike, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use uuid::Uuid;

/// How long a condemned chunk must stay unreferenced before it is deleted.
pub const GRACE: Duration = Duration::hours(72);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct RetentionSettings {
    /// Prune after each successful sync, and at most every 4 h.
    pub automatic: bool,
    /// Keep every version younger than this.
    pub keep_all_hours: u32,
    /// Then the latest per hour until this age.
    pub hourly_until_hours: u32,
    /// Then the latest per day until this age.
    pub daily_until_days: u32,
    /// Then the latest per ISO week until this age; after that, per month.
    pub weekly_until_days: u32,
    /// An untagged side branch idle for longer than this is retired entirely.
    pub branch_expiry_days: Option<u32>,
    /// Budget for stored chunks, enforced after tier thinning.
    pub max_bytes: Option<u64>,
    pub updated_at: DateTime<Utc>,
}

impl Default for RetentionSettings {
    fn default() -> Self {
        Self {
            automatic: true,
            keep_all_hours: 1,
            hourly_until_hours: 24,
            daily_until_days: 7,
            weekly_until_days: 90,
            branch_expiry_days: Some(30),
            max_bytes: None,
            updated_at: DateTime::<Utc>::UNIX_EPOCH,
        }
    }
}

/// Reads the local settings and, when a remote is given, reconciles them
/// last-writer-wins with `<remote>/settings/retention.json` so every device
/// plans with the same rules.
pub fn sync_settings(local: &Path, remote: Option<&Store>) -> Result<RetentionSettings> {
    let local_settings: RetentionSettings = read_json(local).unwrap_or_default();
    let Some(remote) = remote else {
        return Ok(local_settings);
    };
    let remote_path = remote.root().join("settings").join("retention.json");
    let remote_settings = read_json::<RetentionSettings>(&remote_path).ok();
    match remote_settings {
        Some(remote_settings) if remote_settings.updated_at > local_settings.updated_at => {
            write_json(local, &remote_settings)?;
            Ok(remote_settings)
        }
        Some(remote_settings) if remote_settings == local_settings => Ok(local_settings),
        _ => {
            write_json(&remote_path, &local_settings)?;
            Ok(local_settings)
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct Plan {
    /// (game, slot, version) to retire.
    pub retire: Vec<(Uuid, Uuid, Uuid)>,
    pub live_bytes_before: u64,
    pub live_bytes_after: u64,
    /// The protected versions alone exceed the disk budget.
    pub budget_unreachable: bool,
}

pub struct PlanInput<'a> {
    pub games: &'a [GameRecord],
    pub retired: &'a HashSet<Uuid>,
    /// Versions any registered device points at.
    pub pointed: &'a HashSet<Uuid>,
    pub chunk_sizes: &'a HashMap<String, u64>,
    pub settings: &'a RetentionSettings,
    pub now: DateTime<Utc>,
}

/// Decides which versions to retire. Pure and deterministic.
pub fn plan<'a>(input: &PlanInput<'a>) -> Plan {
    let now = input.now;
    let mut retire: HashSet<Uuid> = HashSet::new();
    let mut protected: HashSet<Uuid> = HashSet::new();
    let mut candidates: Vec<(DateTime<Utc>, Uuid, Uuid, Uuid)> = Vec::new();

    for game in input
        .games
        .iter()
        .filter(|game| game.meta.deleted_at.is_none())
    {
        for slot in game
            .slots
            .iter()
            .filter(|slot| slot.meta.deleted_at.is_none())
        {
            let versions: HashMap<Uuid, _> = slot.versions.iter().map(|v| (v.uuid, v)).collect();
            let mut children: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
            let mut roots = Vec::new();
            for version in &slot.versions {
                match version.parent_uuid {
                    Some(parent) if versions.contains_key(&parent) => {
                        children.entry(parent).or_default().push(version.uuid)
                    }
                    _ => roots.push(version.uuid),
                }
            }
            for list in children.values_mut() {
                list.sort_by_key(|uuid| (versions[uuid].created_at, *uuid));
            }
            let is_protected = |uuid: &Uuid| {
                let version = versions[uuid];
                version
                    .tag
                    .as_deref()
                    .is_some_and(|tag| !tag.trim().is_empty())
                    || version
                        .parent_uuid
                        .is_none_or(|parent| !versions.contains_key(&parent))
                    || input.pointed.contains(uuid)
                    || version.created_at > now
            };
            protected.extend(versions.keys().filter(|uuid| is_protected(uuid)));

            // Branch expiry: an idle side subtree with nothing protected in it.
            if let Some(days) = input.settings.branch_expiry_days {
                let cutoff = now - Duration::days(i64::from(days));
                for kids in children.values().filter(|kids| kids.len() > 1) {
                    let newest = |kid: &Uuid| {
                        descendants(*kid, &children)
                            .iter()
                            .map(|uuid| (versions[uuid].created_at, *uuid))
                            .max()
                    };
                    let main_line = kids.iter().max_by_key(|kid| newest(kid)).copied();
                    for kid in kids.iter().filter(|kid| Some(**kid) != main_line) {
                        let subtree = descendants(*kid, &children);
                        let idle = subtree
                            .iter()
                            .all(|uuid| versions[uuid].created_at < cutoff);
                        if idle && !subtree.iter().any(|uuid| protected.contains(uuid)) {
                            retire.extend(subtree);
                        }
                    }
                }
            }

            // Tier thinning per chain segment (between forks).
            for segment in segments(&roots, &children) {
                let mut kept: HashMap<Bucket, (DateTime<Utc>, Uuid)> = HashMap::new();
                let mut losers = Vec::new();
                for uuid in segment {
                    let version = versions[&uuid];
                    if protected.contains(&uuid)
                        || retire.contains(&uuid)
                        || input.retired.contains(&uuid)
                    {
                        continue;
                    }
                    let Some(bucket) = bucket(version.created_at, now, input.settings) else {
                        continue;
                    };
                    let key = (version.created_at, uuid);
                    match kept.get(&bucket) {
                        Some(best) if *best >= key => losers.push(uuid),
                        Some(best) => {
                            losers.push(best.1);
                            kept.insert(bucket, key);
                        }
                        None => {
                            kept.insert(bucket, key);
                        }
                    }
                }
                retire.extend(losers);
            }
            for version in &slot.versions {
                if !input.retired.contains(&version.uuid) && !protected.contains(&version.uuid) {
                    candidates.push((
                        version.created_at,
                        version.uuid,
                        game.meta.uuid,
                        slot.meta.uuid,
                    ));
                }
            }
        }
    }
    retire.retain(|uuid| !protected.contains(uuid) && !input.retired.contains(uuid));

    // Disk budget over the unique live chunks.
    let mut refs: HashMap<&'a str, usize> = HashMap::new();
    let mut version_chunks: HashMap<Uuid, HashSet<&'a str>> = HashMap::new();
    for game in input
        .games
        .iter()
        .filter(|game| game.meta.deleted_at.is_none())
    {
        for slot in game
            .slots
            .iter()
            .filter(|slot| slot.meta.deleted_at.is_none())
        {
            for version in &slot.versions {
                if input.retired.contains(&version.uuid) {
                    continue;
                }
                let chunks: HashSet<&'a str> = version.chunk_hashes().collect();
                for chunk in &chunks {
                    *refs.entry(chunk).or_default() += 1;
                }
                version_chunks.insert(version.uuid, chunks);
            }
        }
    }
    let size = |chunk: &str| input.chunk_sizes.get(chunk).copied().unwrap_or(0);
    let live_bytes_before: u64 = refs.keys().map(|chunk| size(chunk)).sum();
    let mut live_bytes = live_bytes_before;
    let drop_version = |uuid: &Uuid, live_bytes: &mut u64, refs: &mut HashMap<&'a str, usize>| {
        for chunk in version_chunks.get(uuid).into_iter().flatten() {
            let count = refs.get_mut(chunk).expect("counted above");
            *count -= 1;
            if *count == 0 {
                *live_bytes -= size(chunk);
            }
        }
    };
    for uuid in &retire {
        drop_version(uuid, &mut live_bytes, &mut refs);
    }
    let mut budget_unreachable = false;
    if let Some(max) = input.settings.max_bytes {
        candidates.sort();
        let order: Vec<Uuid> = candidates
            .iter()
            .map(|(_, uuid, _, _)| *uuid)
            .filter(|uuid| !retire.contains(uuid))
            .collect();
        let mut next = order.into_iter();
        while live_bytes > max {
            let Some(uuid) = next.next() else {
                budget_unreachable = true;
                break;
            };
            drop_version(&uuid, &mut live_bytes, &mut refs);
            retire.insert(uuid);
        }
    }

    let mut planned: Vec<(Uuid, Uuid, Uuid)> = candidates
        .iter()
        .filter(|(_, uuid, _, _)| retire.contains(uuid))
        .map(|(_, uuid, game, slot)| (*game, *slot, *uuid))
        .collect();
    planned.sort_by_key(|(_, _, uuid)| *uuid);
    Plan {
        retire: planned,
        live_bytes_before,
        live_bytes_after: live_bytes,
        budget_unreachable,
    }
}

/// The subtree below `start`, itself included. Corrupt metadata can make
/// ancestry cyclic, so every node is visited once.
fn descendants(start: Uuid, children: &HashMap<Uuid, Vec<Uuid>>) -> Vec<Uuid> {
    let mut out = Vec::new();
    let mut seen = HashSet::from([start]);
    let mut stack = vec![start];
    while let Some(uuid) = stack.pop() {
        out.push(uuid);
        stack.extend(
            children
                .get(&uuid)
                .into_iter()
                .flatten()
                .filter(|kid| seen.insert(**kid)),
        );
    }
    out
}

/// Splits the graph into chains whose interior versions have one child.
/// Starting from roots is cycle-safe: a version has one parent, so a cycle is
/// never reachable from a root.
fn segments(roots: &[Uuid], children: &HashMap<Uuid, Vec<Uuid>>) -> Vec<Vec<Uuid>> {
    let mut segments = Vec::new();
    let mut starts: Vec<Uuid> = roots.to_vec();
    while let Some(start) = starts.pop() {
        let mut segment = vec![start];
        let mut current = start;
        loop {
            match children.get(&current).map(Vec::as_slice) {
                Some([only]) => {
                    segment.push(*only);
                    current = *only;
                }
                Some(many) => {
                    starts.extend(many);
                    break;
                }
                None => break,
            }
        }
        segments.push(segment);
    }
    segments
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Bucket {
    Hour(i32, u32, u32, u32),
    Day(i32, u32, u32),
    Week(i32, u32),
    Month(i32, u32),
}

/// `None` keeps the version (young, or in the future).
fn bucket(
    created: DateTime<Utc>,
    now: DateTime<Utc>,
    settings: &RetentionSettings,
) -> Option<Bucket> {
    let age = now - created;
    if age < Duration::hours(i64::from(settings.keep_all_hours)) {
        return None;
    }
    if age < Duration::hours(i64::from(settings.hourly_until_hours)) {
        return Some(Bucket::Hour(
            created.year(),
            created.month(),
            created.day(),
            created.hour(),
        ));
    }
    if age < Duration::days(i64::from(settings.daily_until_days)) {
        return Some(Bucket::Day(created.year(), created.month(), created.day()));
    }
    if age < Duration::days(i64::from(settings.weekly_until_days)) {
        let week = created.iso_week();
        return Some(Bucket::Week(week.year(), week.week()));
    }
    Some(Bucket::Month(created.year(), created.month()))
}

/// Writes markers for the planned versions this store contains; returns how many were new.
pub fn apply(store: &Store, plan: &Plan, now: DateTime<Utc>) -> Result<usize> {
    let present: HashSet<Uuid> = store
        .catalog()?
        .iter()
        .flat_map(|game| game.versions().map(|v| v.uuid).collect::<Vec<_>>())
        .collect();
    let mut written = 0;
    for (game, slot, version) in &plan.retire {
        if present.contains(version)
            && store.write_marker(&RetirementMarker {
                version_uuid: *version,
                slot_uuid: *slot,
                game_uuid: *game,
                retired_at: now,
            })?
        {
            written += 1;
        }
    }
    Ok(written)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct GcReport {
    pub chunks_deleted: usize,
    pub bytes_deleted: u64,
    /// Remote only: chunks newly condemned, deleted by a later sweep.
    pub chunks_condemned: usize,
}

/// Chunks of every live version plus of the `extra` versions (device pointers).
fn live_chunks(store: &Store, extra: &HashSet<Uuid>) -> Result<HashSet<String>> {
    if let Some(path) = store.first_unloadable_metadata()? {
        bail!(
            "{} is unloadable; garbage collection is skipped until it is fixed",
            path.display()
        );
    }
    let retired = store.markers()?;
    let mut live = HashSet::new();
    for game in store.catalog()? {
        for slot in &game.slots {
            let slot_live = game.meta.deleted_at.is_none() && slot.meta.deleted_at.is_none();
            for version in &slot.versions {
                if (slot_live && !retired.contains_key(&version.uuid))
                    || extra.contains(&version.uuid)
                {
                    live.extend(version.chunk_hashes().map(str::to_string));
                }
            }
        }
    }
    Ok(live)
}

/// Deletes every unreferenced chunk. The caller must hold the repository lock.
pub fn gc_local(store: &Store, pointed: &HashSet<Uuid>) -> Result<GcReport> {
    let live = live_chunks(store, pointed)?;
    let mut report = GcReport::default();
    for (hash, size) in store.chunk_sizes()? {
        if !live.contains(&hash) {
            let path = store.chunk_path(&hash)?;
            std::fs::remove_file(&path)
                .with_context(|| format!("failed to delete {}", path.display()))?;
            report.chunks_deleted += 1;
            report.bytes_deleted += size;
        }
    }
    Ok(report)
}

/// Bytes a local GC would reclaim now.
pub fn gc_preview(store: &Store, pointed: &HashSet<Uuid>) -> Result<GcReport> {
    let live = live_chunks(store, pointed)?;
    let mut report = GcReport::default();
    for (hash, size) in store.chunk_sizes()? {
        if !live.contains(&hash) {
            report.chunks_deleted += 1;
            report.bytes_deleted += size;
        }
    }
    Ok(report)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Condemned {
    device: Uuid,
    created_at: DateTime<Utc>,
    hashes: Vec<String>,
}

/// Remote GC with several devices and no locks: sweep chunks condemned more
/// than `GRACE` ago that are still unreferenced, then condemn the chunks that
/// are unreferenced now. Publishers re-upload anything swept too eagerly.
pub fn gc_remote(
    store: &Store,
    pointed: &HashSet<Uuid>,
    device: Uuid,
    now: DateTime<Utc>,
) -> Result<GcReport> {
    let live = live_chunks(store, pointed)?;
    let sizes = store.chunk_sizes()?;
    let dir = store.root().join("gc").join("condemned");
    let mut report = GcReport::default();
    let mut pending: HashSet<String> = HashSet::new();
    for path in crate::store::entries(&dir)? {
        if crate::store::json_file_uuid(&path).is_none() {
            continue;
        }
        let Ok(record) = read_json::<Condemned>(&path) else {
            continue;
        };
        if now - record.created_at < GRACE {
            pending.extend(record.hashes);
            continue;
        }
        for hash in record.hashes.iter().filter(|hash| !live.contains(*hash)) {
            if let Some(size) = sizes.get(hash) {
                match std::fs::remove_file(store.chunk_path(hash)?) {
                    Ok(()) => {
                        report.chunks_deleted += 1;
                        report.bytes_deleted += size;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error).context("failed to delete a condemned chunk"),
                }
            }
        }
        let _ = std::fs::remove_file(&path);
    }
    let mut hashes: Vec<String> = sizes
        .keys()
        .filter(|hash| !live.contains(*hash) && !pending.contains(*hash))
        .cloned()
        .collect();
    if !hashes.is_empty() {
        hashes.sort();
        report.chunks_condemned = hashes.len();
        write_json(
            &dir.join(format!("{}.json", Uuid::new_v4())),
            &Condemned {
                device,
                created_at: now,
                hashes,
            },
        )?;
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{FileEntry, GameMeta, SlotMeta, SlotRecord, VersionMeta, VersionReason};

    struct Graph {
        game: GameRecord,
    }

    fn at(hours_ago: i64, now: DateTime<Utc>) -> DateTime<Utc> {
        now - Duration::hours(hours_ago)
    }

    fn hash(n: u8) -> String {
        format!("sha256:{}", format!("{n:02x}").repeat(32))
    }

    impl Graph {
        fn new() -> Self {
            let (game_uuid, slot_uuid) = (Uuid::from_u128(1), Uuid::from_u128(2));
            let t = DateTime::<Utc>::UNIX_EPOCH;
            Self {
                game: GameRecord {
                    meta: GameMeta {
                        uuid: game_uuid,
                        title: "G".into(),
                        folder_slug: "g__g_1".into(),
                        created_at: t,
                        updated_at: t,
                        deleted_at: None,
                        icon_sha256: None,
                        accent_hue: None,
                        accent_hue_source: Default::default(),
                    },
                    tracked_dirs: vec![],
                    slots: vec![SlotRecord {
                        meta: SlotMeta {
                            uuid: slot_uuid,
                            game_uuid,
                            folder_slug: "s__s_2".into(),
                            name: "S".into(),
                            created_at: t,
                            updated_at: t,
                            deleted_at: None,
                            seen_versions: Vec::new(),
                        },
                        versions: vec![],
                    }],
                },
            }
        }

        fn add(
            &mut self,
            id: u128,
            parent: Option<u128>,
            created: DateTime<Utc>,
            chunk: u8,
        ) -> Uuid {
            let uuid = Uuid::from_u128(id);
            let (game_uuid, slot_uuid) = (self.game.meta.uuid, self.game.slots[0].meta.uuid);
            self.game.slots[0].versions.push(VersionMeta {
                uuid,
                game_uuid,
                slot_uuid,
                parent_uuid: parent.map(Uuid::from_u128),
                version_number: id as i32,
                folder_slug: format!("v__v_{id}"),
                reason: VersionReason::Watcher,
                tag: None,
                created_at: created,
                updated_at: created,
                origin_device: None,
                files: vec![FileEntry {
                    tracked_dir_uuid: Uuid::nil(),
                    path: "save".into(),
                    size: 1,
                    sha256: hash(chunk),
                    chunks: vec![hash(chunk)],
                }],
            });
            uuid
        }

        fn plan(&self, pointed: &[Uuid], settings: RetentionSettings, now: DateTime<Utc>) -> Plan {
            let sizes: HashMap<String, u64> = (0..=255u8).map(|n| (hash(n), 100)).collect();
            plan(&PlanInput {
                games: std::slice::from_ref(&self.game),
                retired: &HashSet::new(),
                pointed: &pointed.iter().copied().collect(),
                chunk_sizes: &sizes,
                settings: &settings,
                now,
            })
        }
    }

    fn retired(plan: &Plan) -> HashSet<u128> {
        plan.retire.iter().map(|(_, _, v)| v.as_u128()).collect()
    }

    #[test]
    fn tiers_keep_latest_per_bucket_and_protect_root_tags_and_pointers() {
        let now = DateTime::parse_from_rfc3339("2026-09-27T12:30:00Z")
            .unwrap()
            .to_utc();
        let mut graph = Graph::new();
        graph.add(1, None, at(24 * 40, now), 1); // root, protected
        graph.add(2, Some(1), at(24 * 20, now), 2); // weekly tier
        graph.add(3, Some(2), at(24 * 20 - 1, now), 3); // same ISO week, later → kept
        graph.add(4, Some(3), at(5, now) - Duration::minutes(10), 4); // hourly 07:xx
        graph.add(5, Some(4), at(5, now), 5); // same hour, later → kept
        graph.add(6, Some(5), at(0, now), 6); // keep-all
        let plan = graph.plan(&[Uuid::from_u128(6)], RetentionSettings::default(), now);
        assert_eq!(retired(&plan), HashSet::from([2, 4]));

        graph.game.slots[0].versions[1].tag = Some("boss".into());
        let plan = graph.plan(&[Uuid::from_u128(6)], RetentionSettings::default(), now);
        assert_eq!(retired(&plan), HashSet::from([4]));
        let plan = graph.plan(&[Uuid::from_u128(4)], RetentionSettings::default(), now);
        assert!(
            !retired(&plan).contains(&4),
            "a device pointer is protected"
        );
    }

    #[test]
    fn month_tier_applies_beyond_weekly() {
        let now = DateTime::parse_from_rfc3339("2026-09-27T00:00:00Z")
            .unwrap()
            .to_utc();
        let mut graph = Graph::new();
        graph.add(1, None, at(24 * 200, now), 1);
        graph.add(
            2,
            Some(1),
            DateTime::parse_from_rfc3339("2026-03-02T00:00:00Z")
                .unwrap()
                .to_utc(),
            2,
        );
        graph.add(
            3,
            Some(2),
            DateTime::parse_from_rfc3339("2026-03-20T00:00:00Z")
                .unwrap()
                .to_utc(),
            3,
        );
        graph.add(4, Some(3), at(0, now), 4);
        let plan = graph.plan(&[Uuid::from_u128(4)], RetentionSettings::default(), now);
        assert_eq!(retired(&plan), HashSet::from([2]));
    }

    #[test]
    fn idle_side_branches_expire_but_keep_the_fork_and_tagged_chains() {
        let now = Utc::now();
        let mut graph = Graph::new();
        graph.add(1, None, at(24 * 60, now), 1);
        graph.add(2, Some(1), at(24 * 50, now), 2); // fork
        graph.add(3, Some(2), at(24 * 49, now), 3); // side branch
        graph.add(4, Some(3), at(24 * 48, now), 4);
        graph.add(5, Some(2), at(1, now), 5); // main line, pointed
        let settings = RetentionSettings {
            keep_all_hours: 24 * 365,
            ..RetentionSettings::default()
        };
        let plan = graph.plan(&[Uuid::from_u128(5)], settings, now);
        assert_eq!(retired(&plan), HashSet::from([3, 4]));
        graph.game.slots[0].versions[3].tag = Some("keep".into());
        assert!(retired(&graph.plan(&[Uuid::from_u128(5)], settings, now)).is_empty());
    }

    #[test]
    fn the_newest_line_from_a_fork_never_expires() {
        let now = Utc::now();
        let mut graph = Graph::new();
        graph.add(1, None, at(24 * 60, now), 1);
        graph.add(2, Some(1), at(24 * 55, now), 2); // fork
        graph.add(3, Some(2), at(24 * 54, now), 3);
        graph.add(4, Some(2), at(24 * 40, now), 4); // newer line, nobody points at it
        let settings = RetentionSettings {
            keep_all_hours: 24 * 365,
            ..RetentionSettings::default()
        };
        assert_eq!(retired(&graph.plan(&[], settings, now)), HashSet::from([3]));
    }

    #[test]
    fn budget_retires_oldest_until_met_or_reports_unreachable() {
        let now = Utc::now();
        let mut graph = Graph::new();
        graph.add(1, None, at(10, now), 1);
        for id in 2..=6 {
            graph.add(id, Some(id - 1), at(10 - id as i64, now), id as u8);
        }
        let settings = RetentionSettings {
            keep_all_hours: 1000,
            max_bytes: Some(300),
            ..RetentionSettings::default()
        };
        let plan = graph.plan(&[Uuid::from_u128(6)], settings, now);
        assert_eq!(plan.live_bytes_before, 600);
        assert_eq!(retired(&plan), HashSet::from([2, 3, 4]));
        assert_eq!(plan.live_bytes_after, 300);
        assert!(!plan.budget_unreachable);
        let tight = RetentionSettings {
            max_bytes: Some(100),
            ..settings
        };
        let plan = graph.plan(&[Uuid::from_u128(6)], tight, now);
        assert!(
            plan.budget_unreachable,
            "root and pointer alone exceed the budget"
        );
        assert_eq!(retired(&plan), HashSet::from([2, 3, 4, 5]));
    }

    #[test]
    fn cyclic_ancestry_does_not_hang_branch_expiry() {
        let now = Utc::now();
        let mut graph = Graph::new();
        graph.add(1, Some(2), at(24 * 50, now), 1);
        graph.add(2, Some(1), at(24 * 49, now), 2);
        graph.add(3, Some(1), at(24 * 48, now), 3); // fork off the cycle
        let settings = RetentionSettings {
            keep_all_hours: 24 * 365,
            ..RetentionSettings::default()
        };
        let (done, result) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = done.send(retired(&graph.plan(&[], settings, now)));
        });
        result
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("planning hangs on cyclic ancestry");
    }

    #[test]
    fn gc_refuses_to_run_when_a_version_manifest_is_invalid() {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path()).unwrap();
        let now = Utc::now();
        let game = GameMeta {
            uuid: Uuid::from_u128(1),
            title: "G".into(),
            folder_slug: "g__g_1".into(),
            created_at: now,
            updated_at: now,
            deleted_at: None,
            icon_sha256: None,
            accent_hue: None,
            accent_hue_source: Default::default(),
        };
        let slot = SlotMeta {
            uuid: Uuid::from_u128(2),
            game_uuid: game.uuid,
            folder_slug: "s__s_2".into(),
            name: "S".into(),
            created_at: now,
            updated_at: now,
            deleted_at: None,
            seen_versions: Vec::new(),
        };
        store.write_game(&game).unwrap();
        store.write_slot("g__g_1", &slot).unwrap();
        let chunk = hash(7);
        crate::chunks::publish_chunk(&store, &chunk, b"x").unwrap();
        let version = VersionMeta {
            uuid: Uuid::from_u128(3),
            game_uuid: game.uuid,
            slot_uuid: slot.uuid,
            parent_uuid: None,
            version_number: 1,
            folder_slug: "v__v_3".into(),
            reason: VersionReason::Watcher,
            tag: None,
            created_at: now,
            updated_at: now,
            origin_device: None,
            files: vec![FileEntry {
                tracked_dir_uuid: Uuid::nil(),
                path: "save".into(),
                size: 1,
                sha256: chunk.clone(),
                chunks: vec![chunk.clone()],
            }],
        };
        store.publish_version("g__g_1", "s__s_2", &version).unwrap();
        assert_eq!(store.first_unloadable_metadata().unwrap(), None);

        let path = store
            .game_dir("g__g_1")
            .join("slots/s__s_2/versions/v__v_3/metadata.json");
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, text.replacen(&chunk, "sha256:bad", 1)).unwrap();
        assert_eq!(store.first_unloadable_metadata().unwrap(), Some(path));
        assert!(gc_local(&store, &HashSet::new()).is_err());
        assert!(store.has_chunk(&chunk), "the only chunk must survive");
    }
}

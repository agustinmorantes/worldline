use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Result, anyhow, bail};
use clap::{Parser, Subcommand};
use savegame_timeline_core::bundle::HistoryBundleSummary;
use savegame_timeline_core::detection::Notification;
use savegame_timeline_core::history::CheckpointOutcome;
use savegame_timeline_core::maintenance::RetentionSettings;
use savegame_timeline_core::models::GameForm;
use savegame_timeline_core::sync::SyncSettings;
use savegame_timeline_core::{OpenOptions, TimelineCore};
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "savegame-cli")]
struct Cli {
    #[arg(long)]
    data_dir: Option<PathBuf>,

    /// Debounce seconds for the file watcher (hidden, for testing)
    #[arg(long, hide = true)]
    debounce_secs: Option<f32>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Games {
        #[command(subcommand)]
        action: GamesAction,
    },
    Versions {
        #[command(subcommand)]
        action: VersionsAction,
    },
    Slots {
        #[command(subcommand)]
        action: SlotsAction,
    },
    Prune {
        #[command(subcommand)]
        action: PruneAction,
    },
    History {
        #[command(subcommand)]
        action: HistoryAction,
    },
    Sync {
        #[command(subcommand)]
        action: SyncAction,
    },
    Devices {
        #[command(subcommand)]
        action: DevicesAction,
    },
    /// Repository maintenance and recovery.
    Repo {
        #[command(subcommand)]
        action: RepoAction,
    },
    Watch,
}

#[derive(Subcommand)]
enum GamesAction {
    List,
    Add {
        title: String,
        #[arg(long = "dir")]
        dirs: Vec<String>,
    },
    Edit {
        id: i32,
        title: String,
    },
    Show {
        id: i32,
    },
    Delete {
        id: i32,
    },
}

#[derive(Subcommand)]
enum VersionsAction {
    /// Capture the live saves now (no-op when unchanged).
    Checkpoint {
        game: String,
    },
    Tag {
        version_id: i32,
        tag: String,
    },
    Restore {
        version_id: i32,
    },
}

#[derive(Subcommand)]
enum SlotsAction {
    List {
        game: String,
    },
    Create {
        game: String,
        name: String,
    },
    Switch {
        game: String,
        slot: String,
    },
    Rename {
        game: String,
        slot: String,
        new_name: String,
    },
    Delete {
        game: String,
        slot: String,
        #[arg(long)]
        replacement: Option<String>,
    },
}

#[derive(Subcommand)]
enum PruneAction {
    /// Show or change the retention settings.
    Settings {
        #[arg(long)]
        automatic: Option<bool>,
        #[arg(long)]
        keep_all_hours: Option<u32>,
        #[arg(long)]
        hourly_until_hours: Option<u32>,
        #[arg(long)]
        daily_until_days: Option<u32>,
        #[arg(long)]
        weekly_until_days: Option<u32>,
        /// Days after which an idle side branch is retired; 0 disables.
        #[arg(long)]
        branch_expiry_days: Option<u32>,
        /// Disk budget in bytes; 0 removes the budget.
        #[arg(long)]
        max_bytes: Option<u64>,
    },
    Preview,
    Run,
}

#[derive(Subcommand)]
enum HistoryAction {
    Inspect {
        bundle_dir: PathBuf,
    },
    Export {
        game: String,
        destination_dir: PathBuf,
    },
    Import {
        bundle_dir: PathBuf,
        #[arg(long = "map", value_name = "TRACKED-DIR-UUID=ABSOLUTE-PATH")]
        mappings: Vec<String>,
    },
}

#[derive(Subcommand)]
enum SyncAction {
    /// Show the sync folder and each game's state, without syncing.
    Status,
    /// Set the sync folder (`--off` disables sync).
    Configure {
        folder: Option<PathBuf>,
        #[arg(long, conflicts_with = "folder")]
        off: bool,
    },
    /// Pull, adopt, fast-forward and push.
    Refresh,
    /// Publish local history only.
    Push,
    /// Start tracking a game that exists in the sync folder.
    Adopt { game_uuid: Uuid },
}

#[derive(Subcommand)]
enum DevicesAction {
    List,
    Remove { device_uuid: Uuid },
}

#[derive(Subcommand)]
enum RepoAction {
    /// Verify manifests and chunks (`--deep` decompresses every chunk).
    Check {
        #[arg(long)]
        deep: bool,
    },
    /// Rebuild a version's files into a folder, without touching live saves.
    Extract {
        version_id: i32,
        destination: PathBuf,
    },
    /// Rebuild the database index from the repository files.
    Reindex,
}

fn default_data_dir() -> Result<PathBuf> {
    // Same path Tauri resolves for app_local_data_dir on each platform
    let local_data = dirs::data_local_dir()
        .ok_or_else(|| anyhow!("could not determine local data directory"))?;
    Ok(local_data.join("agustinmorantes.savegame-timeline"))
}

fn resolve_game_id(core: &TimelineCore, game: &str) -> Result<i32> {
    if let Ok(id) = game.parse() {
        return Ok(id);
    }
    core.get_games()?
        .into_iter()
        .find(|candidate| candidate.game.title == game || candidate.game.uuid == game)
        .map(|candidate| candidate.game.id)
        .ok_or_else(|| anyhow!("Game '{game}' not found"))
}

fn resolve_slot_id(core: &TimelineCore, game_id: i32, slot: &str) -> Result<i32> {
    let slots = core.get_game(game_id)?.slots;
    if let Ok(id) = slot.parse()
        && slots.iter().any(|candidate| candidate.id == id)
    {
        return Ok(id);
    }
    slots
        .into_iter()
        .find(|candidate| candidate.name == slot || candidate.uuid == slot)
        .map(|candidate| candidate.id)
        .ok_or_else(|| anyhow!("Slot '{slot}' not found for game {game_id}"))
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if let Commands::History {
        action: HistoryAction::Inspect { bundle_dir },
    } = &cli.command
    {
        print_history_summary(&TimelineCore::inspect_history_bundle(bundle_dir)?);
        return Ok(());
    }
    let data_dir = match cli.data_dir {
        Some(p) => p,
        None => default_data_dir()?,
    };
    let (core, notifications) = TimelineCore::open(
        &data_dir,
        OpenOptions {
            watch: matches!(cli.command, Commands::Watch),
            debounce_seconds: cli.debounce_secs,
        },
    )?;
    if let Some(backup) = core.moved_old_data() {
        eprintln!(
            "Data from a pre-release version was moved to {} and the app started fresh.",
            backup.display()
        );
    }

    match cli.command {
        Commands::Games { action } => match action {
            GamesAction::List => {
                let games = core.get_games()?;
                if games.is_empty() {
                    println!("No games.");
                }
                for g in games {
                    println!(
                        "[{}] {} — {} version(s), {} tracked dir(s)",
                        g.game.id,
                        g.game.title,
                        g.versions.len(),
                        g.tracked_dirs.len()
                    );
                }
            }
            GamesAction::Edit { id, title } => {
                let game = core.save_game(
                    GameForm {
                        id: Some(id),
                        title,
                    },
                    None,
                )?;
                println!("Updated game [{}] {}", game.id, game.title);
            }
            GamesAction::Add { title, dirs } => {
                let game = core.save_game(GameForm { id: None, title }, Some(dirs))?;
                println!("Added game [{}] {}", game.id, game.title);
            }
            GamesAction::Show { id } => {
                let g = core.get_game(id)?;
                println!("[{}] {} ({})", g.game.id, g.game.title, g.game.uuid);
                println!("  Tracked dirs:");
                for d in &g.tracked_dirs {
                    println!("    [{}] {}", d.id, d.path);
                }
                println!("  Versions ({}):", g.versions.len());
                for v in &g.versions {
                    let tag = v.tag.as_deref().unwrap_or("-");
                    let retired = if v.retired { " (retired)" } else { "" };
                    println!(
                        "    [{}] #{} at {} tag={}{retired}",
                        v.id, v.version_number, v.created_at, tag
                    );
                }
            }
            GamesAction::Delete { id } => {
                core.delete_game(id)?;
                println!("Deleted game {id}");
            }
        },
        Commands::Versions { action } => match action {
            VersionsAction::Checkpoint { game } => {
                let game_id = resolve_game_id(&core, &game)?;
                match core.checkpoint(game_id)? {
                    CheckpointOutcome::Created(v) => {
                        println!("Created version [{}] #{}", v.id, v.version_number)
                    }
                    CheckpointOutcome::Unchanged(v) => {
                        println!("Unchanged; version [{}] is current", v.id)
                    }
                }
            }
            VersionsAction::Tag { version_id, tag } => {
                let v = core.tag_version(version_id, tag)?;
                println!("Tagged version [{}] as {:?}", v.id, v.tag);
            }
            VersionsAction::Restore { version_id } => {
                core.restore_version(version_id)?;
                println!("Restored version {version_id}");
            }
        },
        Commands::Slots { action } => match action {
            SlotsAction::List { game } => {
                let full = core.get_game(resolve_game_id(&core, &game)?)?;
                for slot in &full.slots {
                    let marker = if full.active_slot.as_ref().is_some_and(|a| a.id == slot.id) {
                        "*"
                    } else {
                        " "
                    };
                    println!("{marker} [{}] {}", slot.id, slot.name);
                }
            }
            SlotsAction::Create { game, name } => {
                let full = core.create_slot(resolve_game_id(&core, &game)?, &name)?;
                let slot = full
                    .active_slot
                    .ok_or_else(|| anyhow!("No active slot after creation"))?;
                println!("Created slot [{}] {}", slot.id, slot.name);
            }
            SlotsAction::Switch { game, slot } => {
                let game_id = resolve_game_id(&core, &game)?;
                let slot_id = resolve_slot_id(&core, game_id, &slot)?;
                let slot = core
                    .switch_slot(game_id, slot_id)?
                    .active_slot
                    .ok_or_else(|| anyhow!("No active slot after switch"))?;
                println!("Switched to slot [{}] {}", slot.id, slot.name);
            }
            SlotsAction::Rename {
                game,
                slot,
                new_name,
            } => {
                let game_id = resolve_game_id(&core, &game)?;
                let slot_id = resolve_slot_id(&core, game_id, &slot)?;
                let slot = core
                    .rename_slot(slot_id, &new_name)?
                    .slots
                    .into_iter()
                    .find(|s| s.id == slot_id)
                    .ok_or_else(|| anyhow!("Renamed slot not found"))?;
                println!("Renamed slot [{}] to {}", slot.id, slot.name);
            }
            SlotsAction::Delete {
                game,
                slot,
                replacement,
            } => {
                let game_id = resolve_game_id(&core, &game)?;
                let slot_id = resolve_slot_id(&core, game_id, &slot)?;
                let replacement = replacement
                    .as_deref()
                    .map(|slot| resolve_slot_id(&core, game_id, slot))
                    .transpose()?;
                core.delete_slot(slot_id, replacement)?;
                println!("Deleted slot {slot_id}");
            }
        },
        Commands::Prune { action } => match action {
            PruneAction::Settings {
                automatic,
                keep_all_hours,
                hourly_until_hours,
                daily_until_days,
                weekly_until_days,
                branch_expiry_days,
                max_bytes,
            } => {
                let mut settings = core.get_pruning_settings()?;
                let before = settings;
                settings.automatic = automatic.unwrap_or(settings.automatic);
                settings.keep_all_hours = keep_all_hours.unwrap_or(settings.keep_all_hours);
                settings.hourly_until_hours =
                    hourly_until_hours.unwrap_or(settings.hourly_until_hours);
                settings.daily_until_days = daily_until_days.unwrap_or(settings.daily_until_days);
                settings.weekly_until_days =
                    weekly_until_days.unwrap_or(settings.weekly_until_days);
                if let Some(days) = branch_expiry_days {
                    settings.branch_expiry_days = (days > 0).then_some(days);
                }
                if let Some(bytes) = max_bytes {
                    settings.max_bytes = (bytes > 0).then_some(bytes);
                }
                if settings != before {
                    settings = core.update_pruning_settings(settings)?;
                }
                print_retention(&settings);
            }
            PruneAction::Preview => {
                let preview = core.preview_pruning()?;
                println!(
                    "{} checkpoint(s) eligible for retirement",
                    preview.retire_versions.len()
                );
                println!(
                    "Stored history: {} byte(s) now, {} byte(s) after retirement",
                    preview.live_bytes_before, preview.live_bytes_after
                );
                if preview.budget_unreachable {
                    println!(
                        "The disk budget can't be met without removing protected checkpoints."
                    );
                }
                println!(
                    "{} chunk(s), {} byte(s) already reclaimable locally",
                    preview.local_gc.chunks_deleted, preview.local_gc.bytes_deleted
                );
            }
            PruneAction::Run => {
                let report = core.run_pruning()?;
                println!(
                    "Retired {} checkpoint(s); reclaimed {} local chunk(s), {} byte(s)",
                    report.retired_versions.len(),
                    report.local_gc.chunks_deleted,
                    report.local_gc.bytes_deleted
                );
                if let Some(remote) = report.remote_gc {
                    println!(
                        "Remote: deleted {} chunk(s), {} byte(s); {} chunk(s) scheduled for deletion",
                        remote.chunks_deleted, remote.bytes_deleted, remote.chunks_condemned
                    );
                }
                if report.budget_unreachable {
                    println!(
                        "The disk budget can't be met without removing protected checkpoints."
                    );
                }
            }
        },
        Commands::History { action } => match action {
            HistoryAction::Inspect { .. } => unreachable!("handled before opening the core"),
            HistoryAction::Export {
                game,
                destination_dir,
            } => {
                let report =
                    core.export_game_history(resolve_game_id(&core, &game)?, &destination_dir)?;
                println!(
                    "Exported {}: {} slot(s), {} version(s), {} chunk(s), {} retired version(s)",
                    report.title,
                    report.slot_count,
                    report.version_count,
                    report.blob_count,
                    report.retired_version_count
                );
            }
            HistoryAction::Import {
                bundle_dir,
                mappings: values,
            } => {
                let mut mappings = HashMap::new();
                for value in values {
                    let (uuid, path) = value.split_once('=').ok_or_else(|| {
                        anyhow!("Invalid mapping {value:?}; expected UUID=ABSOLUTE_PATH")
                    })?;
                    let uuid = Uuid::parse_str(uuid)
                        .map_err(|error| anyhow!("Invalid tracked-directory UUID: {error}"))?;
                    if mappings.insert(uuid, PathBuf::from(path)).is_some() {
                        bail!("Duplicate mapping for tracked-directory UUID {uuid}");
                    }
                }
                let report = core.import_game_history(&bundle_dir, &mappings)?;
                println!(
                    "Imported {}: {} slot(s), {} version(s), {} chunk(s), {} retired version(s)",
                    report.title,
                    report.slot_count,
                    report.version_count,
                    report.blob_count,
                    report.retired_version_count
                );
            }
        },
        Commands::Sync { action } => match action {
            SyncAction::Status => match core.get_sync_settings()?.remote_path {
                None => println!("Sync is off."),
                Some(path) => {
                    println!("Sync folder: {path}");
                    print_sync_report(&core.sync_status()?);
                }
            },
            SyncAction::Configure { folder, off } => {
                let remote_path = if off {
                    None
                } else {
                    Some(
                        folder
                            .ok_or_else(|| anyhow!("Give a folder, or --off"))?
                            .to_string_lossy()
                            .into_owned(),
                    )
                };
                let settings = core.update_sync_settings(SyncSettings { remote_path })?;
                match settings.remote_path {
                    Some(path) => println!("Sync folder set to {path}"),
                    None => println!("Sync is off."),
                }
            }
            SyncAction::Refresh => print_sync_report(&core.refresh_sync()?),
            SyncAction::Push => {
                core.push_sync()?;
                println!("Pushed.");
            }
            SyncAction::Adopt { game_uuid } => {
                let game = core.adopt_remote_game(game_uuid)?;
                println!(
                    "Adopted [{}] {} — {} version(s)",
                    game.game.id,
                    game.game.title,
                    game.versions.len()
                );
            }
        },
        Commands::Devices { action } => match action {
            DevicesAction::List => {
                let me = core.device_metadata();
                println!("This device: {} ({})", me.name, me.device_uuid);
                if core.get_sync_settings()?.remote_path.is_some() {
                    for device in core.sync_status()?.devices {
                        if device.device_uuid != me.device_uuid {
                            println!(
                                "  {} ({}) — last seen {}",
                                device.name, device.device_uuid, device.last_seen
                            );
                        }
                    }
                }
            }
            DevicesAction::Remove { device_uuid } => {
                core.remove_sync_device(device_uuid)?;
                println!("Removed device {device_uuid}");
            }
        },
        Commands::Repo { action } => match action {
            RepoAction::Check { deep } => {
                let report = core.check_integrity(deep)?;
                println!(
                    "{} version(s), {} chunk(s) checked; {} missing, {} corrupt, {} unreferenced",
                    report.versions_checked,
                    report.chunks_checked,
                    report.missing_chunks.len(),
                    report.corrupt_chunks.len(),
                    report.orphan_chunks
                );
                if let Some(path) = &report.unreadable_metadata {
                    println!("Unreadable metadata: {}", path.display());
                }
                if !report.is_ok() {
                    bail!("The repository has problems");
                }
            }
            RepoAction::Extract {
                version_id,
                destination,
            } => {
                let files = core.extract_version(version_id, &destination)?;
                println!("Extracted {files} file(s) to {}", destination.display());
            }
            RepoAction::Reindex => {
                core.reindex()?;
                println!("Index rebuilt from {}", core.repository_root().display());
            }
        },
        Commands::Watch => {
            println!("Watching for changes (Ctrl+C to stop)...");
            for notification in notifications {
                match notification {
                    Notification::ChangeDetected { game_id } => {
                        println!("Change detected for game {game_id}")
                    }
                    Notification::VersionCreated {
                        game_id,
                        version_number,
                    } => {
                        println!("Version {version_number} created for game {game_id}");
                        if let Err(error) = core.push_sync() {
                            eprintln!("Sync error: {error:#}");
                        }
                    }
                    Notification::Error(err) => eprintln!("Error: {err}"),
                }
            }
        }
    }

    Ok(())
}

fn print_retention(settings: &RetentionSettings) {
    println!("Automatic: {}", settings.automatic);
    println!(
        "Keep all for {} h; hourly until {} h; daily until {} d; weekly until {} d; monthly after",
        settings.keep_all_hours,
        settings.hourly_until_hours,
        settings.daily_until_days,
        settings.weekly_until_days
    );
    match settings.branch_expiry_days {
        Some(days) => println!("Idle side branches expire after {days} d"),
        None => println!("Side branches never expire"),
    }
    match settings.max_bytes {
        Some(bytes) => println!("Disk budget: {bytes} byte(s)"),
        None => println!("No disk budget"),
    }
}

fn print_sync_report(report: &savegame_timeline_core::sync::SyncReport) {
    for game in &report.remote_games {
        let status = report.games.iter().find(|s| s.game_uuid == game.uuid);
        let relation = match status {
            Some(status) => format!("{:?}", status.relation),
            None if game.adopted => "Unknown".into(),
            None => "Not tracked here".into(),
        };
        println!("{} ({}) — {relation}", game.title, game.uuid);
    }
    for problem in &report.problems {
        println!("! {problem}");
    }
}

fn print_history_summary(summary: &HistoryBundleSummary) {
    println!("{} ({})", summary.title, summary.game_uuid);
    println!(
        "{} slot(s), {} version(s), {} tracked directory(s), {} chunk(s), {} retired version(s)",
        summary.slot_count,
        summary.version_count,
        summary.tracked_dir_count,
        summary.blob_count,
        summary.retired_version_count
    );
    for tracked in &summary.active_tracked_dirs {
        println!(
            "  {}{} — source hint: {}",
            tracked.uuid,
            tracked
                .label
                .as_deref()
                .map(|label| format!(" ({label})"))
                .unwrap_or_default(),
            tracked.source_path_hint
        );
    }
}

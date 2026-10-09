use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Result, anyhow, bail};
use clap::{Parser, Subcommand};
use uuid::Uuid;
use worldline_core::bundle::HistoryBundleSummary;
use worldline_core::detection::Notification;
use worldline_core::history::CheckpointOutcome;
use worldline_core::maintenance::RetentionSettings;
use worldline_core::models::GameForm;
use worldline_core::sync::DeviceKind;
use worldline_core::sync::SyncSettings;
use worldline_core::{OpenOptions, WorldlineCore};

#[derive(Parser)]
#[command(name = "worldline-cli")]
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
    /// Sync with the sync folder. Only needed when neither the app nor `watch` is running.
    Sync {
        #[command(subcommand)]
        action: Option<SyncAction>,
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
    /// Screenshots a running game's window repeatedly and prints one CSV line
    /// per shot (profiling only).
    #[cfg(windows)]
    #[command(hide = true)]
    ScreenshotStress {
        #[arg(long)]
        game: i32,
        /// Seconds between shots.
        #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u64).range(1..))]
        every: u64,
        /// How many shots; 0 runs until Ctrl+C.
        #[arg(long, default_value_t = 0)]
        count: u32,
        /// Where each shot is written (overwritten every time).
        #[arg(long, default_value = "screenshot.jpg")]
        out: PathBuf,
    },
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
    /// Stop tracking a game on this device; its history stays in the sync folder.
    Untrack {
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
    /// Start tracking a game that exists in the sync folder.
    Adopt { game_uuid: Uuid },
}

#[derive(Subcommand)]
enum DevicesAction {
    List,
    /// Rename this device and set its kind; an omitted value is kept.
    Set {
        #[arg(long)]
        name: Option<String>,
        #[arg(long, value_enum)]
        kind: Option<KindArg>,
    },
    Remove {
        device_uuid: Uuid,
    },
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum KindArg {
    Desktop,
    Laptop,
    Handheld,
}

impl From<KindArg> for DeviceKind {
    fn from(kind: KindArg) -> Self {
        match kind {
            KindArg::Desktop => DeviceKind::Desktop,
            KindArg::Laptop => DeviceKind::Laptop,
            KindArg::Handheld => DeviceKind::Handheld,
        }
    }
}

/// How a device's kind reads in listings; unknown reads as desktop.
fn kind_label(kind: Option<DeviceKind>) -> &'static str {
    match kind {
        None | Some(DeviceKind::Desktop) => "desktop",
        Some(DeviceKind::Laptop) => "laptop",
        Some(DeviceKind::Handheld) => "handheld",
    }
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
    Ok(local_data.join("agustinmorantes.worldline"))
}

fn resolve_game_id(core: &WorldlineCore, game: &str) -> Result<i32> {
    if let Ok(id) = game.parse() {
        return Ok(id);
    }
    core.get_games()?
        .into_iter()
        .find(|candidate| candidate.game.title == game || candidate.game.uuid == game)
        .map(|candidate| candidate.game.id)
        .ok_or_else(|| anyhow!("Game '{game}' not found"))
}

fn resolve_slot_id(core: &WorldlineCore, game_id: i32, slot: &str) -> Result<i32> {
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

fn report_moved_data(core: &WorldlineCore) {
    if let Some(backup) = core.moved_old_data() {
        eprintln!(
            "Data from a pre-release version was moved to {} and the app started fresh.",
            backup.display()
        );
    }
}

/// Captures, syncs and prunes in the background until Ctrl+C, which drains
/// pending captures and pushes them before exiting.
fn watch(data_dir: &std::path::Path, debounce_seconds: Option<f32>) -> Result<()> {
    let (core, notifications) = WorldlineCore::open_shared(
        data_dir,
        OpenOptions {
            watch: true,
            debounce_seconds,
        },
    )?;
    report_moved_data(&core);
    let stopping = core.clone();
    ctrlc::set_handler(move || stopping.shutdown())?;
    println!("Watching for changes (Ctrl+C to stop)...");
    for notification in notifications {
        match notification {
            Notification::ChangeDetected { game_id } => {
                println!("Change detected for game {game_id}")
            }
            Notification::VersionCreated {
                game_id,
                version_number,
            } => println!("Version {version_number} created for game {game_id}"),
            Notification::ScreenshotAttached {
                game_id,
                version_id,
            } => println!("Screenshot saved for version id {version_id} of game {game_id}"),
            Notification::Error(err) => eprintln!("Error: {err}"),
            Notification::SyncStarted => {}
            Notification::SyncCompleted(report) => {
                println!("Synced.");
                print_sync_report(&report);
            }
            Notification::SyncFailed(error) => eprintln!("Sync error: {}", error.message),
            Notification::HistoryPruned => println!("Old versions were pruned."),
            Notification::PruningFailed(error) => eprintln!("Pruning error: {}", error.message),
            Notification::GameStarted { game_id } => println!("Game {game_id} is running"),
            Notification::GameStopped { game_id } => println!("Game {game_id} stopped"),
        }
    }
    Ok(())
}

/// Takes a shot every `every` seconds on this thread, which `Capturer`
/// switches to background priority.
#[cfg(windows)]
fn screenshot_stress(
    install_dir: &str,
    every: u64,
    count: u32,
    out: &std::path::Path,
) -> Result<()> {
    use worldline_core::screenshot::{self, Capturer, Shot};
    if !screenshot::supported() {
        bail!("Screenshots need Windows 11");
    }
    let started = std::time::Instant::now();
    let capturer = Capturer::new()?;
    eprintln!("device ready in {:.1} ms", ms(started.elapsed()));
    println!(
        "unix_ms,qpc,status,src_w,src_h,pids_ms,window_ms,session_ms,first_frame_ms,readback_ms,resize_ms,encode_ms,write_ms,total_ms,cpu_ms,bytes,working_set_mb,peak_working_set_mb"
    );
    let mut taken = 0;
    while count == 0 || taken < count {
        let next = std::time::Instant::now() + std::time::Duration::from_secs(every);
        let unix_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_millis();
        let qpc = screenshot::qpc();
        // The real worker gets the PIDs from `RunningMonitor`; the scan is
        // timed to show what that saves.
        let start = std::time::Instant::now();
        let pids = screenshot::pids_under(install_dir);
        let pids_time = start.elapsed();
        let (status, t, write_time) = match capturer.shot(&pids) {
            Ok(Shot::Taken(jpeg, t)) => {
                let start = std::time::Instant::now();
                let temp = out.with_extension("jpg.tmp");
                std::fs::write(&temp, &jpeg)?;
                std::fs::rename(&temp, out)?;
                ("saved", t, start.elapsed())
            }
            Ok(Shot::Skipped(reason, t)) => (reason, t, Default::default()),
            Err(e) => {
                eprintln!("shot failed: {e:#}");
                ("error", Default::default(), Default::default())
            }
        };
        let mb = |bytes: usize| bytes as f64 / 1_048_576.0;
        println!(
            "{unix_ms},{qpc},{status},{},{},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{},{:.1},{:.1}",
            t.source.0,
            t.source.1,
            ms(pids_time),
            ms(t.window),
            ms(t.session),
            ms(t.first_frame),
            ms(t.readback),
            ms(t.resize),
            ms(t.encode),
            ms(write_time),
            ms(pids_time + t.total + write_time),
            ms(t.cpu),
            t.bytes,
            mb(t.working_set),
            mb(t.peak_working_set),
        );
        taken += 1;
        std::thread::sleep(next.saturating_duration_since(std::time::Instant::now()));
    }
    Ok(())
}

#[cfg(windows)]
fn ms(d: std::time::Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if let Commands::History {
        action: HistoryAction::Inspect { bundle_dir },
    } = &cli.command
    {
        print_history_summary(&WorldlineCore::inspect_history_bundle(bundle_dir)?);
        return Ok(());
    }
    let data_dir = match cli.data_dir {
        Some(p) => p,
        None => default_data_dir()?,
    };
    if let Commands::Watch = cli.command {
        return watch(&data_dir, cli.debounce_secs);
    }
    let (core, _) = WorldlineCore::open(
        &data_dir,
        OpenOptions {
            watch: false,
            debounce_seconds: cli.debounce_secs,
        },
    )?;
    report_moved_data(&core);

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
                if let Some(dir) = &g.game.install_dir {
                    println!("  Installed in: {dir}");
                }
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
            GamesAction::Untrack { id } => {
                core.untrack_game(id)?;
                println!("Untracked game {id}; its history stays in the sync folder");
            }
        },
        Commands::Versions { action } => match action {
            VersionsAction::Checkpoint { game } => {
                let game_id = resolve_game_id(&core, &game)?;
                match core.checkpoint(game_id)? {
                    CheckpointOutcome::Created(v) => {
                        println!("Created version [{}] #{}", v.id, v.version_number)
                    }
                    CheckpointOutcome::Empty => println!("Nothing to capture yet"),
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
                let preview = core.preview_pruning(&core.get_pruning_settings()?)?;
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
            None => print_sync_report(&core.refresh_sync()?),
            Some(SyncAction::Status) => match core.get_sync_settings()?.remote_path {
                None => println!("Sync is off."),
                Some(path) => {
                    println!("Sync folder: {path}");
                    print_sync_report(&core.sync_status()?);
                }
            },
            Some(SyncAction::Configure { folder, off }) => {
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
            Some(SyncAction::Adopt { game_uuid }) => {
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
                println!(
                    "This device: {} ({}), {}",
                    me.name,
                    me.device_uuid,
                    kind_label(me.kind)
                );
                if core.get_sync_settings()?.remote_path.is_some() {
                    for device in core.sync_status()?.devices {
                        if device.device_uuid != me.device_uuid {
                            println!(
                                "  {} ({}), {} — last seen {}",
                                device.name,
                                device.device_uuid,
                                kind_label(device.kind),
                                device.last_seen
                            );
                        }
                    }
                }
            }
            DevicesAction::Set { name, kind } => {
                let me = core.device_metadata();
                let device = core.update_device(
                    name.as_deref().unwrap_or(&me.name),
                    kind.map(DeviceKind::from).or(me.kind),
                )?;
                println!("This device: {} ({})", device.name, kind_label(device.kind));
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
                    "{} version(s), {} chunk(s) checked; {} missing, {} corrupt, {} damaged file(s), {} unreferenced",
                    report.versions_checked,
                    report.chunks_checked,
                    report.missing_chunks.len(),
                    report.corrupt_chunks.len(),
                    report.damaged_files.len(),
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
        Commands::Watch => unreachable!("handled before opening the core"),
        #[cfg(windows)]
        Commands::ScreenshotStress {
            game,
            every,
            count,
            out,
        } => {
            let install_dir = core
                .get_game(game)?
                .game
                .install_dir
                .ok_or_else(|| anyhow!("game {game} has no install dir"))?;
            screenshot_stress(&install_dir, every, count, &out)?;
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

fn print_sync_report(report: &worldline_core::sync::SyncReport) {
    for game in &report.remote_games {
        let status = report.games.iter().find(|s| s.game_uuid == game.uuid);
        let relation = match status {
            Some(status) => format!("{:?}", status.relation),
            None if game.adopted => "Unknown".into(),
            None => "Not tracked here".into(),
        };
        let source = game
            .source_device
            .as_deref()
            .map(|device| format!(", last played on {device}"))
            .unwrap_or_default();
        println!(
            "{} ({}) — {relation}; {} version(s){source}",
            game.title, game.uuid, game.version_count
        );
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

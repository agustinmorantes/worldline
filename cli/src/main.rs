use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use clap::{Parser, Subcommand};
use diesel::SqliteConnection;
use diesel::r2d2::{ConnectionManager, Pool};
use diesel_migrations::MigrationHarness;
use savegame_timeline_core::detection::{Notification, NotificationReceiver};
use savegame_timeline_core::models::game::GameForm;
use savegame_timeline_core::persistence::sqlite::{
    SqliteGameRepository, SqliteScanRootsRepository, SqliteVersionRepository,
};
use savegame_timeline_core::{MIGRATIONS, TimelineCore};
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
    Watch,
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
enum PruneAction {
    Preview,
    Run,
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
    Tag { version_id: i32, tag: String },
    Restore { version_id: i32 },
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

fn init_core_with_debounce(
    data_dir: &Path,
    debounce_secs: Option<f32>,
) -> Result<(TimelineCore, NotificationReceiver)> {
    std::fs::create_dir_all(data_dir)?;
    let db_path = data_dir.join("database.db");
    let database_url = format!("file:{}?foreign_keys=on", db_path.display());
    let manager = ConnectionManager::<SqliteConnection>::new(&database_url);
    let pool = Pool::builder().build(manager)?;
    pool.get()?
        .run_pending_migrations(MIGRATIONS)
        .expect("migrations failed");
    let working_base = data_dir.join("repository");
    let games_repo = Arc::new(SqliteGameRepository::with_repository_root(
        pool.clone(),
        working_base.clone(),
    ));
    let versions_repo = Arc::new(SqliteVersionRepository::with_repository_root(
        pool.clone(),
        working_base.clone(),
    ));
    let scan_roots_repo = Arc::new(SqliteScanRootsRepository::new(pool.clone()));
    let icons_dir = data_dir.join("icons");
    TimelineCore::new(
        games_repo,
        versions_repo,
        scan_roots_repo,
        working_base,
        icons_dir,
        debounce_secs,
    )
}

fn default_data_dir() -> Result<PathBuf> {
    // Same path Tauri resolves for app_local_data_dir on each platform
    let local_data = dirs::data_local_dir()
        .ok_or_else(|| anyhow::anyhow!("could not determine local data directory"))?;
    Ok(local_data.join("agustinmorantes.savegame-timeline"))
}

fn resolve_game_id(core: &TimelineCore, game: &str) -> Result<i32> {
    if let Ok(id) = game.parse() {
        return Ok(id);
    }

    core.get_games()?
        .into_iter()
        .find(|candidate| candidate.game.title == game)
        .map(|candidate| candidate.game.id)
        .ok_or_else(|| anyhow::anyhow!("Game '{game}' not found"))
}

fn resolve_slot_id(core: &TimelineCore, game_id: i32, slot: &str) -> Result<i32> {
    let slots = core.get_slots(game_id)?;
    if let Ok(id) = slot.parse()
        && slots.iter().any(|candidate| candidate.slot.id == id)
    {
        return Ok(id);
    }

    slots
        .into_iter()
        .find(|candidate| candidate.slot.name == slot || candidate.slot.uuid == slot)
        .map(|candidate| candidate.slot.id)
        .ok_or_else(|| anyhow::anyhow!("Slot '{slot}' not found for game {game_id}"))
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
    let (core, notif_rx) = init_core_with_debounce(&data_dir, cli.debounce_secs)?;

    match cli.command {
        Commands::Games { action } => match action {
            GamesAction::List => {
                let games = core.get_games()?;
                if games.is_empty() {
                    println!("No games.");
                } else {
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
            }
            GamesAction::Edit { id, title } => {
                let game = core.save_game(
                    GameForm {
                        id: Some(id),
                        uuid: None,
                        title: &title,
                        title_normalized: None,
                        folder_slug: None,
                        active_slot_id: None,
                        icon_path: None,
                    },
                    None,
                )?;
                println!("Updated game [{}] {}", game.id, game.title);
            }
            GamesAction::Add { title, dirs } => {
                let dir_refs: Vec<&str> = dirs.iter().map(|s| s.as_str()).collect();
                let game = core.save_game(
                    GameForm {
                        id: None,
                        uuid: None,
                        title: &title,
                        title_normalized: None,
                        folder_slug: None,
                        active_slot_id: None,
                        icon_path: None,
                    },
                    if dir_refs.is_empty() {
                        None
                    } else {
                        Some(dir_refs)
                    },
                )?;
                println!("Added game [{}] {}", game.id, game.title);
            }
            GamesAction::Show { id } => {
                let g = core.get_game(id)?;
                println!("[{}] {}", g.game.id, g.game.title);
                println!("  Tracked dirs:");
                for d in &g.tracked_dirs {
                    println!("    [{}] {}", d.id, d.path);
                }
                println!("  Versions ({}):", g.versions.len());
                for v in &g.versions {
                    let tag = v.tag.as_deref().unwrap_or("-");
                    println!(
                        "    [{}] #{} at {} tag={}",
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
                let game_id = resolve_game_id(&core, &game)?;
                let full = core.get_game(game_id)?;
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
                let game_id = resolve_game_id(&core, &game)?;
                let full = core.create_slot(game_id, &name)?;
                let slot = full
                    .active_slot
                    .ok_or_else(|| anyhow::anyhow!("No active slot after creation"))?;
                println!("Created slot [{}] {}", slot.id, slot.name);
            }
            SlotsAction::Switch { game, slot } => {
                let game_id = resolve_game_id(&core, &game)?;
                let slot_id = resolve_slot_id(&core, game_id, &slot)?;
                let full = core.switch_slot(game_id, slot_id)?;
                let slot = full
                    .active_slot
                    .ok_or_else(|| anyhow::anyhow!("No active slot after switch"))?;
                println!("Switched to slot [{}] {}", slot.id, slot.name);
            }
            SlotsAction::Rename {
                game,
                slot,
                new_name,
            } => {
                let game_id = resolve_game_id(&core, &game)?;
                let slot_id = resolve_slot_id(&core, game_id, &slot)?;
                let full = core.rename_slot(slot_id, &new_name)?;
                let slot = full
                    .slots
                    .into_iter()
                    .find(|s| s.id == slot_id)
                    .ok_or_else(|| anyhow::anyhow!("Renamed slot not found"))?;
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
            PruneAction::Preview => {
                let preview = core.preview_pruning()?;
                println!(
                    "{} checkpoint(s) eligible for retirement",
                    preview.retire_versions.len()
                );
                println!(
                    "{} blob(s), {} byte(s) already reclaimable locally",
                    preview.local_gc.reclaimable_blobs, preview.local_gc.reclaimable_bytes
                );
            }
            PruneAction::Run => {
                let report = core.run_pruning()?;
                println!(
                    "Retired {} checkpoint(s); reclaimed {} local blob(s), {} byte(s)",
                    report.retired_versions.len(),
                    report.local_gc.reclaimable_blobs,
                    report.local_gc.reclaimable_bytes
                );
                if let Some(remote) = report.remote_gc {
                    println!(
                        "Reclaimed {} remote blob(s), {} byte(s)",
                        remote.reclaimable_blobs, remote.reclaimable_bytes
                    );
                }
            }
        },
        Commands::History { action } => match action {
            HistoryAction::Inspect { bundle_dir } => {
                print_history_summary(&TimelineCore::inspect_history_bundle(&bundle_dir)?);
            }
            HistoryAction::Export {
                game,
                destination_dir,
            } => {
                let game_id = resolve_game_id(&core, &game)?;
                let report = core.export_game_history(game_id, &destination_dir)?;
                println!(
                    "Exported {}: {} slot(s), {} version(s), {} blob(s), {} retired version(s)",
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
                        anyhow::anyhow!("Invalid mapping {value:?}; expected UUID=ABSOLUTE_PATH")
                    })?;
                    let uuid = Uuid::parse_str(uuid).map_err(|error| {
                        anyhow::anyhow!("Invalid tracked-directory UUID: {error}")
                    })?;
                    if mappings.insert(uuid, PathBuf::from(path)).is_some() {
                        anyhow::bail!("Duplicate mapping for tracked-directory UUID {uuid}");
                    }
                }
                let report = core.import_game_history(&bundle_dir, &mappings)?;
                println!(
                    "Imported {}: {} slot(s), {} version(s), {} blob(s), {} retired version(s)",
                    report.title,
                    report.slot_count,
                    report.version_count,
                    report.blob_count,
                    report.retired_version_count
                );
            }
        },
        Commands::Watch => {
            println!("Watching for changes (Ctrl+C to stop)...");
            for notification in notif_rx {
                match notification {
                    Notification::ChangeDetected { game_id } => {
                        println!("Change detected for game {game_id}");
                    }
                    Notification::VersionCreated {
                        game_id,
                        version_number,
                    } => {
                        println!("Version {version_number} created for game {game_id}");
                        if core
                            .get_sync_settings()
                            .is_ok_and(|settings| settings.remote_path.is_some())
                            && let Err(error) = core.push_sync()
                        {
                            eprintln!("Sync error: {error}");
                        }
                    }
                    Notification::Error(err) => {
                        eprintln!("Error: {err}");
                    }
                }
            }
        }
    }

    Ok(())
}

fn print_history_summary(summary: &savegame_timeline_core::history_transfer::HistoryBundleSummary) {
    println!("{} ({})", summary.title, summary.game_uuid);
    println!(
        "{} slot(s), {} version(s), {} tracked directory(s), {} blob(s), {} retired version(s)",
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

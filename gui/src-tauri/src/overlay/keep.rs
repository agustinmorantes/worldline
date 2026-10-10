//! The keep both hotkeys share (handoff, "Hotkeys"): capture the game's saves at the press and tag
//! that version with the automatic name, which keeps it forever. The name's template comes in the
//! app language with the config (ADR 0007); only the time is filled in here.

use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_log::log::warn;
use worldline_core::detection::{BranchFrom, CaptureFailure};
use worldline_core::history::{CheckpointOutcome, Kept};
use worldline_core::models::Version;
use worldline_core::WorldlineCore;

/// The automatic tag: `template` with `{time}` as the local time, `21:04`.
fn tag(template: &str, time: chrono::NaiveTime) -> String {
    template.replace("{time}", &time.format("%H:%M").to_string())
}

/// Why a keep didn't keep: the capture failed, or it made version number `captured` and then
/// couldn't tag it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotKept {
    pub reason: CaptureFailure,
    pub captured: Option<i32>,
}

/// What a keep tells the app's views and the player.
#[derive(Debug)]
struct Report {
    /// `version-created`: a version was captured, whether or not it was then tagged.
    created: Option<(i32, Option<BranchFrom>)>,
    /// `version-kept`: the id of the version, tagged.
    kept: Option<i32>,
    result: Result<Version, NotKept>,
}

fn report(game_id: i32, outcome: anyhow::Result<Kept>) -> Report {
    let kept = match outcome {
        Ok(kept) => kept,
        Err(err) => {
            warn!("overlay: couldn't keep game {game_id}'s checkpoint: {err:#}");
            let reason = CaptureFailure::from(&err);
            return Report {
                created: None,
                kept: None,
                result: Err(NotKept {
                    reason,
                    captured: None,
                }),
            };
        }
    };
    let created = match &kept.checkpoint {
        CheckpointOutcome::Created(version) => Some((version.version_number, kept.branch_from)),
        _ => None,
    };
    let result = match kept.tagged {
        Some(Ok(version)) => Ok(version),
        Some(Err(err)) => {
            warn!("overlay: captured game {game_id}'s checkpoint but couldn't tag it: {err:#}");
            Err(NotKept {
                reason: CaptureFailure::from(&err),
                captured: created.as_ref().map(|(number, _)| *number),
            })
        }
        None => {
            warn!("overlay: game {game_id} has no saves to keep yet");
            Err(NotKept {
                reason: CaptureFailure::Other,
                captured: None,
            })
        }
    };
    Report {
        created,
        kept: result.as_ref().ok().map(|version| version.id),
        result,
    }
}

/// Keeps game `game_id`'s checkpoint as it is now and tells the app's views. The version comes
/// back tagged, or with the tag it already had; otherwise why it wasn't kept, which includes a
/// game with nothing to capture yet and a version captured but left untagged. `None` when `still`,
/// asked under the repository lock, said the press no longer stands: nothing happened.
pub fn keep(
    app: &AppHandle,
    game_id: i32,
    template: &str,
    still: impl FnOnce() -> bool,
) -> Option<Result<Version, NotKept>> {
    let core = app.state::<Arc<WorldlineCore>>();
    let tag = tag(template, chrono::Local::now().time());
    let report = report(game_id, core.keep(game_id, &tag, still).transpose()?);
    if let Some(created) = report.created {
        let (version_number, branch_from) = created;
        app.emit("version-created", (game_id, version_number, branch_from))
            .ok();
    }
    if let Some(version_id) = report.kept {
        app.emit("version-kept", (game_id, version_id)).ok();
    }
    Some(report.result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_automatic_tag_is_the_pushed_template_with_the_local_time() {
        let at = |h, m| chrono::NaiveTime::from_hms_opt(h, m, 59).unwrap();
        assert_eq!(
            tag("Kept in game, {time}", at(21, 4)),
            "Kept in game, 21:04"
        );
        assert_eq!(
            tag("Conservado en el juego, {time}", at(9, 30)),
            "Conservado en el juego, 09:30"
        );
    }

    fn version(id: i32, number: i32, tag: Option<&str>) -> Version {
        Version {
            id,
            uuid: String::new(),
            slot_id: 1,
            parent_id: None,
            version_number: number,
            folder_slug: String::new(),
            reason: "manual".into(),
            tag: tag.map(Into::into),
            retired: false,
            created_at: chrono::NaiveDateTime::default(),
            origin_device: None,
        }
    }

    fn kept(checkpoint: CheckpointOutcome, tagged: Option<anyhow::Result<Version>>) -> Kept {
        Kept {
            checkpoint,
            branch_from: None,
            tagged,
        }
    }

    #[test]
    fn a_created_and_tagged_version_is_created_and_kept() {
        let report = report(
            3,
            Ok(kept(
                CheckpointOutcome::Created(version(40, 7, None)),
                Some(Ok(version(40, 7, Some("Kept")))),
            )),
        );
        assert_eq!(report.created.map(|(number, _)| number), Some(7));
        assert_eq!(report.kept, Some(40));
        assert_eq!(report.result.unwrap().tag.as_deref(), Some("Kept"));
    }

    #[test]
    fn a_version_captured_but_not_tagged_is_still_created_and_reported_as_not_kept() {
        let report = report(
            3,
            Ok(kept(
                CheckpointOutcome::Created(version(40, 7, None)),
                Some(Err(anyhow::anyhow!("index refresh failed"))),
            )),
        );
        // The views refresh to the version, and the popup says it wasn't kept, naming it.
        assert_eq!(report.created.map(|(number, _)| number), Some(7));
        assert_eq!(report.kept, None);
        assert_eq!(
            report.result.unwrap_err(),
            NotKept {
                reason: CaptureFailure::Other,
                captured: Some(7),
            }
        );
    }

    #[test]
    fn an_unchanged_version_is_kept_without_a_new_one_and_failures_capture_nothing() {
        let unchanged = report(
            3,
            Ok(kept(
                CheckpointOutcome::Unchanged(version(40, 7, None)),
                Some(Ok(version(40, 7, Some("Kept")))),
            )),
        );
        assert!(unchanged.created.is_none());
        assert_eq!(unchanged.kept, Some(40));

        // A tag failing on an unchanged version captured nothing new.
        let untagged = report(
            3,
            Ok(kept(
                CheckpointOutcome::Unchanged(version(40, 7, None)),
                Some(Err(anyhow::anyhow!("disk full"))),
            )),
        );
        let empty = report(3, Ok(kept(CheckpointOutcome::Empty, None)));
        let failed = report(3, Err(anyhow::anyhow!("no space")));
        for report in [untagged, empty, failed] {
            assert!(report.created.is_none() && report.kept.is_none());
            assert_eq!(report.result.unwrap_err().captured, None);
        }
    }
}

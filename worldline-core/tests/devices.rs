//! What the GUI shows about devices and playthroughs: who created each
//! version, device kinds, remote games' origin and per-slot summaries.

mod common;

use common::Device;
use uuid::Uuid;
use worldline_core::sync::DeviceKind;

#[test]
fn versions_carry_the_device_that_created_them() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    let a_uuid = a.core.device_metadata().device_uuid.to_string();
    a.write("save.dat", "1");
    let game = a.add_game("Origins");
    a.write("save.dat", "2");
    a.checkpoint(game.game.id);
    assert!(
        a.game(game.game.id)
            .versions
            .iter()
            .all(|v| v.origin_device.as_deref() == Some(a_uuid.as_str()))
    );
    a.core.refresh_sync().unwrap();

    let b = Device::synced(remote.path());
    let b_uuid = b.core.device_metadata().device_uuid.to_string();
    let on_b = b
        .core
        .adopt_remote_game(Uuid::parse_str(&game.game.uuid).unwrap())
        .unwrap();
    assert!(
        on_b.versions
            .iter()
            .all(|v| v.origin_device.as_deref() == Some(a_uuid.as_str())),
        "pulled versions keep their origin"
    );
    a.write("save.dat", "3");
    let made_on_b = b.checkpoint(on_b.game.id);
    assert_eq!(made_on_b.origin_device.as_deref(), Some(b_uuid.as_str()));
    b.core.push_sync().unwrap();

    a.core.refresh_sync().unwrap();
    let on_a = a.game(game.game.id);
    let pulled = on_a
        .versions
        .iter()
        .find(|v| v.uuid == made_on_b.uuid)
        .unwrap();
    assert_eq!(pulled.origin_device.as_deref(), Some(b_uuid.as_str()));
    a.core.reindex().unwrap();
    assert_eq!(a.game(game.game.id).versions, on_a.versions);
}

#[test]
fn every_slot_has_a_summary() {
    let device = Device::new();
    device.write("save.dat", "1");
    let game = device.add_game("Playthroughs");
    device.write("save.dat", "2");
    device.checkpoint(game.game.id);
    device.core.create_slot(game.game.id, "New game+").unwrap();
    device.write("save.dat", "3");
    device.checkpoint(game.game.id);

    let full = device.game(game.game.id);
    assert_eq!(full.slots.len(), 2);
    assert_eq!(full.slot_summaries.len(), 2);
    for (slot, summary) in full.slots.iter().zip(&full.slot_summaries) {
        let versions = device.core.get_slot(slot.id).unwrap().versions;
        assert_eq!(summary.slot_id, slot.id);
        assert_eq!(summary.version_count as usize, versions.len());
        assert_eq!(
            summary.last_created_at,
            versions.iter().map(|v| v.created_at).max()
        );
    }
    let listed = device.core.get_games().unwrap();
    assert_eq!(listed[0].slot_summaries, full.slot_summaries);
}

#[test]
fn the_device_kind_is_published_and_remote_games_show_their_origin() {
    let remote = tempfile::tempdir().unwrap();
    let a = Device::synced(remote.path());
    assert_eq!(a.core.device_metadata().kind, None);
    let me = a
        .core
        .update_device("  Deck ", Some(DeviceKind::Handheld))
        .unwrap();
    assert_eq!(
        (me.name.as_str(), me.kind),
        ("Deck", Some(DeviceKind::Handheld))
    );
    assert!(a.core.update_device(" ", None).is_err());
    a.write("save.dat", "1");
    let game = a.add_game("Travelling");
    a.write("save.dat", "2");
    a.checkpoint(game.game.id);
    a.core.refresh_sync().unwrap();

    let b = Device::synced(remote.path());
    let report = b.core.refresh_sync().unwrap();
    let deck = report
        .devices
        .iter()
        .find(|d| d.device_uuid == me.device_uuid)
        .unwrap();
    assert_eq!(
        (deck.name.as_str(), deck.kind),
        ("Deck", Some(DeviceKind::Handheld))
    );
    let listed = report
        .remote_games
        .iter()
        .find(|g| g.title == "Travelling")
        .unwrap();
    assert!(!listed.adopted);
    assert_eq!(listed.source_device.as_deref(), Some("Deck"));
    assert_eq!(listed.version_count, 2);
}

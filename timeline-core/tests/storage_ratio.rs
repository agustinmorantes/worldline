//! NFR 4.2.2.1: history must take less than 50% of storing every version as
//! a full copy. Three fixtures cover the cases that matter (ADR 0003); each
//! prints its measured ratio.

mod common;

use common::Device;
use std::fs;
use std::path::Path;

const VERSIONS: usize = 10;
const EIGHT_MIB: usize = 8 * 1024 * 1024;

fn repository_bytes(root: &Path) -> u64 {
    let mut total = 0;
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        let metadata = fs::symlink_metadata(&path).unwrap();
        total += if metadata.is_dir() {
            repository_bytes(&path)
        } else {
            metadata.len()
        };
    }
    total
}

/// Deterministic, incompressible bytes.
fn noise(seed: u64, len: usize) -> Vec<u8> {
    let mut state = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 24) as u8
        })
        .collect()
}

/// Runs `versions` captures, where `write(device, n)` produces version n and
/// returns the size of a full copy of the tracked files.
fn ratio(name: &str, write: impl Fn(&Device, usize) -> u64) -> f64 {
    let device = Device::new();
    let mut full_copies = write(&device, 0);
    let game = device.add_game(name);
    for n in 1..VERSIONS {
        full_copies += write(&device, n);
        device.checkpoint(game.game.id);
    }
    assert_eq!(device.game(game.game.id).versions.len(), VERSIONS);
    let stored = repository_bytes(device.core.repository_root());
    let ratio = stored as f64 / full_copies as f64;
    println!(
        "{name}: {stored} bytes stored for {full_copies} bytes of full copies = {:.1}%",
        ratio * 100.0
    );
    ratio
}

#[test]
fn single_large_save_with_small_edits() {
    let base = noise(1, EIGHT_MIB);
    let ratio = ratio("small edits", |device, n| {
        let mut bytes = base.clone();
        for edit in 0..n {
            let at = (edit * 1_048_573 + n * 7919) % (EIGHT_MIB - 64);
            bytes[at..at + 64].copy_from_slice(&noise(n as u64 * 100 + edit as u64, 64));
        }
        device.write("save.dat", "");
        fs::write(device.save_dir().join("save.dat"), &bytes).unwrap();
        bytes.len() as u64
    });
    assert!(ratio < 0.5, "ratio {ratio}");
}

#[test]
fn many_files_with_one_changed() {
    let ratio = ratio("one of many", |device, n| {
        let mut total = 0;
        for file in 0..50 {
            let seed = if file == n % 50 {
                1000 + n as u64
            } else {
                file as u64
            };
            let bytes = noise(seed, 64 * 1024);
            total += bytes.len() as u64;
            fs::create_dir_all(device.save_dir()).unwrap();
            fs::write(device.save_dir().join(format!("slot{file:02}.sav")), bytes).unwrap();
        }
        total
    });
    assert!(ratio < 0.5, "ratio {ratio}");
}

#[test]
fn single_large_save_fully_rewritten() {
    let ratio = ratio("full rewrite", |device, n| {
        let mut text = String::with_capacity(EIGHT_MIB + 128);
        let mut i = 0u64;
        while text.len() < EIGHT_MIB {
            let value = noise(n as u64 * 1_000_003 + i, 4);
            text.push_str(&format!(
                "{{\"entity\":{i},\"kind\":\"npc\",\"x\":{},\"y\":{},\"hp\":{},\"flags\":[false,true]}}\n",
                value[0], value[1], value[2]
            ));
            i += 1;
        }
        fs::create_dir_all(device.save_dir()).unwrap();
        fs::write(device.save_dir().join("world.json"), &text).unwrap();
        text.len() as u64
    });
    assert!(ratio < 0.5, "ratio {ratio}");
}

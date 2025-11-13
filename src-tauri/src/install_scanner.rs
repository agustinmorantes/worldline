use std::fs;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct InstallScannerItem {
    pub title: String,
    pub dirs: Vec<String>,
    pub icon: String,
}

pub fn scan_installed() -> Vec<InstallScannerItem> {
    // MOCKED DATA
    // TODO: take from ludusavi manifest
    let games = vec![
        InstallScannerItem {
            title: "Hollow Knight".to_owned(),
            dirs: vec!["${localappdata}low\\Team Cherry\\Hollow Knight".to_owned()],
            icon: "hollow_knight".to_owned()
        },
        InstallScannerItem {
            title: "Celeste".to_owned(),
            dirs: vec![
                "${programfiles(x86)}\\Steam\\steamapps\\common\\Celeste\\Saves".to_owned(),
                "E:\\SteamLibrary\\steamapps\\common\\Celeste\\Saves".to_owned()
            ],
            icon: "celeste".to_owned()
        },
        InstallScannerItem {
            title: "Balatro".to_owned(),
            dirs: vec!["${appdata}\\Balatro".to_owned()],
            icon: "balatro".to_owned()
        },
    ];

    games.iter()
        .map(|g| {
            let dirs = g.dirs.iter().map(|d| shellexpand::full(d).unwrap().to_string()).collect();

            InstallScannerItem {
                title: g.title.clone(),
                icon: g.icon.clone(),
                dirs,
            }
        })
        .filter(|g| {
            for dir in &g.dirs {
                if fs::metadata(dir).map(|m| m.is_dir()).unwrap_or(false) {
                    return true
                }
            }

            false
        }).collect()
}

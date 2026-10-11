use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};

const BUNDLED_MANIFEST: &[u8] = include_bytes!("../assets/manifest.yaml");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum GameSource {
    Steam,
    Epic,
    Local,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct InstallScannerItem {
    pub title: String,
    pub dirs: Vec<String>,
    #[serde(default)]
    pub candidate_dirs: Vec<String>,
    pub install_path: Option<String>,
    pub icon_path: Option<String>,
    pub last_modified_at: Option<i64>,
    pub source: GameSource,
}

// Structs for parsing Ludusavi manifest.yaml
#[derive(Debug, Deserialize)]
pub struct Manifest {
    #[serde(flatten)]
    pub games: HashMap<String, ManifestGame>,
}

#[derive(Debug, Deserialize)]
pub struct ManifestGame {
    pub files: Option<HashMap<String, ManifestFilePath>>,
    #[serde(rename = "installDir")]
    pub install_dir: Option<HashMap<String, serde::de::IgnoredAny>>,
    pub steam: Option<SteamMeta>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct ManifestFilePath {
    pub tags: Option<Vec<String>>,
    pub when: Option<Vec<WhenCondition>>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct WhenCondition {
    pub os: Option<String>,
    pub store: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SteamMeta {
    pub id: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct SecondaryManifest {
    pub files: Option<HashMap<String, ManifestFilePath>>,
}

#[derive(Debug, Deserialize)]
struct EpicManifest {
    #[serde(rename = "DisplayName")]
    display_name: String,
    #[serde(rename = "InstallLocation")]
    install_location: String,
    #[serde(rename = "LaunchExecutable")]
    launch_executable: Option<String>,
}

// ---------------------------------------------------------
// Helper functions
// ---------------------------------------------------------

fn val_as_obj<'a>(v: &'a keyvalues_parser::Value<'a>) -> Option<&'a keyvalues_parser::Obj<'a>> {
    match v {
        keyvalues_parser::Value::Obj(obj) => Some(obj),
        _ => None,
    }
}

fn val_as_str<'a>(v: &'a keyvalues_parser::Value<'a>) -> Option<&'a str> {
    match v {
        keyvalues_parser::Value::Str(cow) => Some(cow.as_ref()),
        _ => None,
    }
}

fn touch_file(path: &Path) -> std::io::Result<()> {
    let now = std::time::SystemTime::now();
    let file = fs::OpenOptions::new().write(true).open(path)?;
    file.set_modified(now)?;
    Ok(())
}

#[allow(clippy::collapsible_if)]
fn load_manifest(cache_dir: &Path) -> Manifest {
    let cache_yaml = cache_dir.join("manifest.yaml");
    let cache_etag = cache_dir.join("manifest.etag");

    let mut use_cache = false;
    if cache_yaml.exists() {
        if let Ok(metadata) = fs::metadata(&cache_yaml) {
            if let Ok(modified) = metadata.modified() {
                if let Ok(elapsed) = modified.elapsed() {
                    if elapsed.as_secs() < 24 * 3600 {
                        use_cache = true;
                    }
                }
            }
        }
    }

    if !use_cache {
        let etag = if cache_etag.exists() {
            fs::read_to_string(&cache_etag)
                .unwrap_or_default()
                .trim()
                .trim_matches('"')
                .to_string()
        } else {
            String::new()
        };

        let url = "https://raw.githubusercontent.com/mtkennerly/ludusavi-manifest/master/data/manifest.yaml";
        let mut req = ureq::get(url);
        if !etag.is_empty() {
            req = req.set("If-None-Match", &format!("\"{}\"", etag));
        }

        match req.call() {
            Ok(resp) => {
                let new_etag = resp
                    .header("ETag")
                    .unwrap_or("")
                    .trim()
                    .trim_matches('"')
                    .to_string();
                let mut reader = resp.into_reader();
                if let Err(e) = fs::create_dir_all(cache_dir) {
                    eprintln!("[install_scanner] failed to create cache dir: {e}");
                } else {
                    match File::create(&cache_yaml) {
                        Ok(mut file) => match std::io::copy(&mut reader, &mut file) {
                            Ok(_bytes) => {
                                let _ = fs::write(&cache_etag, new_etag);
                            }
                            Err(e) => {
                                eprintln!("[install_scanner] failed to stream manifest: {e}");
                                let _ = fs::remove_file(&cache_yaml);
                            }
                        },
                        Err(e) => eprintln!("[install_scanner] failed to create cache file: {e}"),
                    }
                }
            }
            Err(ureq::Error::Status(304, _)) => {
                let _ = touch_file(&cache_yaml);
            }
            Err(e) => eprintln!("[install_scanner] manifest download failed: {e}"),
        }
    }

    if cache_yaml.exists() {
        if let Ok(file) = File::open(&cache_yaml) {
            let reader = BufReader::new(file);
            if let Ok(manifest) = serde_yaml_ng::from_reader::<_, Manifest>(reader) {
                return manifest;
            }
            eprintln!("[install_scanner] cache manifest parse failed, falling back to bundled");
        }
    }

    let reader = BufReader::new(BUNDLED_MANIFEST);
    serde_yaml_ng::from_reader(reader).unwrap_or_else(|_| Manifest {
        games: HashMap::new(),
    })
}

#[allow(clippy::collapsible_if)]
pub fn resolve_placeholders(
    template: &str,
    root: Option<&str>,
    game: Option<&str>,
    store_user_id: Option<&str>,
    store_game_id: Option<&str>,
) -> String {
    let mut path = crate::path_placeholders::expand_path_placeholders(template);

    if path.contains("<winDir>") {
        let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".to_string());
        path = path.replace("<winDir>", &windir);
    }

    if let Some(r) = root {
        path = path.replace("<root>", r);
    }
    if let Some(g) = game {
        path = path.replace("<game>", g);
    }
    if let Some(r) = root {
        if let Some(g) = game {
            let base = std::path::Path::new(r).join(g);
            path = path.replace("<base>", &base.to_string_lossy());
        }
    }
    if let Some(uid) = store_user_id {
        path = path.replace("<storeUserId>", uid);
    }
    if let Some(gid) = store_game_id {
        path = path.replace("<storeGameId>", gid);
    }

    #[cfg(target_os = "windows")]
    {
        path = path.replace("/", "\\");
    }
    #[cfg(not(target_os = "windows"))]
    {
        path = path.replace("\\", "/");
    }

    path
}

/// The entry that the path component holding the wildcard at `wildcard`
/// matched, in the first match of `pattern`: `Vampire_Survivors_123` for
/// `.../Vampire_Survivors_*/SaveData.sav`.
fn matched_component(pattern: &str, wildcard: usize) -> Option<PathBuf> {
    let end = pattern[wildcard..]
        .find(['/', '\\'])
        .map_or(pattern.len(), |offset| wildcard + offset);
    let depth = Path::new(&pattern[..end]).components().count();
    let first = glob::glob(&pattern.replace('\\', "/"))
        .ok()?
        .filter_map(Result::ok)
        .next()?;
    Some(first.components().take(depth).collect())
}

/// The folder to track for a resolved manifest path, if it exists. Never a
/// folder other apps share.
fn resolve_tracked_directory(resolved_path: &str) -> Option<PathBuf> {
    resolve_existing_directory(resolved_path)
        .filter(|dir| !crate::path_placeholders::is_shared_root(dir))
}

fn resolve_existing_directory(resolved_path: &str) -> Option<PathBuf> {
    // 1. Literal path check
    let path = PathBuf::from(resolved_path);
    if path.is_dir() {
        return Some(path);
    } else if path.exists() {
        return path.parent().map(|p| p.to_path_buf());
    }

    // 2. Glob path check
    let idx = resolved_path.find(['*', '?', '['])?;
    let before_glob = &resolved_path[..idx];
    let base_path = PathBuf::from(before_glob.trim_end_matches(['/', '\\']));
    if base_path.is_dir() && !crate::path_placeholders::is_shared_root(&base_path) {
        // Specific game directory exists; no need to check glob matches (e.g. GameName/*.sav).
        Some(base_path)
    } else if before_glob.ends_with(['/', '\\']) && !base_path.is_dir() {
        // Glob is inside a directory that doesn't exist.
        None
    } else {
        // Glob in a name (e.g. My Games/Super DX-Ball *, Game/save*.dat): it
        // must match something, and what it matched decides the folder.
        let entry = matched_component(resolved_path, idx)?;
        if entry.is_dir() {
            Some(entry)
        } else {
            entry.parent().map(Path::to_path_buf)
        }
    }
}

/// Where the saves of a resolved manifest path would be, before they exist.
/// Never a folder other apps share.
fn candidate_tracked_directory(resolved_path: &str) -> Option<PathBuf> {
    let candidate = if let Some(index) = resolved_path.find(['*', '?', '[']) {
        let prefix = resolved_path[..index].trim_end_matches(['/', '\\']);
        let path = PathBuf::from(prefix);
        if resolved_path[..index].ends_with(['/', '\\']) {
            path
        } else {
            path.parent().unwrap_or(&path).to_path_buf()
        }
    } else {
        let path = PathBuf::from(resolved_path);
        if path.extension().is_some() {
            path.parent().unwrap_or(&path).to_path_buf()
        } else {
            path
        }
    };
    (!crate::path_placeholders::is_shared_root(&candidate)).then_some(candidate)
}

fn is_save_path(tags: &Option<Vec<String>>) -> bool {
    if let Some(t) = tags {
        if t.is_empty() {
            true
        } else {
            t.contains(&"save".to_string())
        }
    } else {
        true
    }
}

fn find_ico_file(dir: &Path) -> Option<String> {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) == Some("ico") {
                return Some(path.to_string_lossy().to_string());
            }
        }
    }
    None
}

fn is_non_game_exe(filename: &str) -> bool {
    let lower = filename.to_lowercase();
    // Skip known crash handlers, uninstallers, and redistributables
    lower.contains("crashhandler")
        || lower.contains("crashreportclient")
        || matches!(
            lower.as_str(),
            "setup.exe"
                | "install.exe"
                | "uninstall.exe"
                | "vcredist_x64.exe"
                | "vcredist_x86.exe"
                | "dxsetup.exe"
                | "directx.exe"
                | "dotnetfx.exe"
        )
        || lower.starts_with("unins")
        || lower.starts_with("vcredist")
}

fn find_largest_exe(dir: &Path) -> Option<String> {
    let mut best: Option<(u64, PathBuf)> = None;
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let is_exe = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("exe"))
                .unwrap_or(false);
            if is_exe
                && !is_non_game_exe(filename)
                && let Ok(meta) = entry.metadata()
            {
                let size = meta.len();
                if best.as_ref().is_none_or(|(s, _)| size > *s) {
                    best = Some((size, path));
                }
            }
        }
    }
    best.map(|(_, p)| p.to_string_lossy().into_owned())
}

fn max_mtime_in_dirs(dirs: &[String]) -> Option<i64> {
    use std::time::UNIX_EPOCH;
    let mut best: Option<i64> = None;
    for dir in dirs {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                if let Ok(meta) = entry.metadata()
                    && let Ok(modified) = meta.modified()
                    && let Ok(dur) = modified.duration_since(UNIX_EPOCH)
                {
                    let secs = dur.as_secs() as i64;
                    if best.is_none_or(|b| secs > b) {
                        best = Some(secs);
                    }
                }
            }
        }
    }
    best
}

// ---------------------------------------------------------
// Pass 1 - Store Enumerations
// ---------------------------------------------------------

#[cfg(target_os = "windows")]
#[allow(clippy::collapsible_if)]
fn get_steam_path_from_registry() -> Option<PathBuf> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(steam_key) = hkcu.open_subkey("Software\\Valve\\Steam") {
        if let Ok(path_str) = steam_key.get_value::<String, _>("SteamPath") {
            let p = PathBuf::from(path_str);
            if p.exists() {
                return Some(p);
            }
        }
    }
    None
}

fn find_steam_root() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        if let Some(p) = get_steam_path_from_registry() {
            return Some(p);
        }
        let fallback_paths = ["C:\\Program Files (x86)\\Steam", "C:\\Program Files\\Steam"];
        for path in &fallback_paths {
            let p = PathBuf::from(path);
            if p.exists() {
                return Some(p);
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        if let Some(home) = dirs::home_dir() {
            let paths = [home.join(".local/share/Steam"), home.join(".steam/steam")];
            for p in &paths {
                if p.exists() {
                    return Some(p.clone());
                }
            }
        }
    }
    None
}

#[allow(clippy::collapsible_if)]
fn parse_library_folders(steam_root: &Path) -> Vec<PathBuf> {
    let mut roots = vec![steam_root.to_path_buf()];
    let path = steam_root.join("steamapps").join("libraryfolders.vdf");
    if !path.exists() {
        return roots;
    }
    if let Ok(content) = fs::read_to_string(path) {
        if let Ok(parsed) = keyvalues_parser::parse(&content) {
            let vdf = keyvalues_parser::Vdf::from(parsed);
            if let Some(obj) = val_as_obj(&vdf.value) {
                for values in obj.values() {
                    for val in values {
                        if let Some(folder_obj) = val_as_obj(val) {
                            if let Some(path_vals) = folder_obj.get("path") {
                                if let Some(path_str) =
                                    path_vals.first().and_then(|v| val_as_str(v))
                                {
                                    let p = PathBuf::from(path_str);
                                    if p.exists() && !roots.contains(&p) {
                                        roots.push(p);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    roots
}

fn parse_acf_file(acf_path: &Path) -> Option<(u64, String, String)> {
    let content = fs::read_to_string(acf_path).ok()?;
    let parsed = keyvalues_parser::parse(&content).ok()?;
    let vdf = keyvalues_parser::Vdf::from(parsed);
    let root_obj = val_as_obj(&vdf.value)?;

    let appid_vals = root_obj.get("appid")?;
    let appid_str = val_as_str(appid_vals.first()?)?;
    let appid = appid_str.parse::<u64>().ok()?;

    let name_vals = root_obj.get("name")?;
    let name = val_as_str(name_vals.first()?)?.to_string();

    let installdir_vals = root_obj.get("installdir")?;
    let installdir = val_as_str(installdir_vals.first()?)?.to_string();

    Some((appid, name, installdir))
}

#[allow(clippy::collapsible_if)]
fn detect_steam_user_id(steam_root: &Path) -> Option<String> {
    let userdata_dir = steam_root.join("userdata");
    if !userdata_dir.exists() {
        return None;
    }
    let mut subdirs = Vec::new();
    if let Ok(entries) = fs::read_dir(userdata_dir) {
        for entry in entries.flatten() {
            if let Ok(file_type) = entry.file_type() {
                if file_type.is_dir() {
                    let name = entry.file_name();
                    let name_str = name.to_string_lossy();
                    if name_str.chars().all(|c| c.is_ascii_digit()) {
                        if let Ok(metadata) = entry.metadata() {
                            if let Ok(modified) = metadata.modified() {
                                subdirs.push((name_str.into_owned(), modified));
                            }
                        }
                    }
                }
            }
        }
    }
    if subdirs.is_empty() {
        None
    } else if subdirs.len() == 1 {
        Some(subdirs[0].0.clone())
    } else {
        subdirs.sort_by_key(|entry| std::cmp::Reverse(entry.1));
        Some(subdirs[0].0.clone())
    }
}

#[allow(clippy::collapsible_if)]
fn resolve_steam_installs(manifest: &Manifest, results: &mut HashMap<String, InstallScannerItem>) {
    let Some(steam_root) = find_steam_root() else {
        return;
    };
    let lib_roots = parse_library_folders(&steam_root);
    let store_user_id = detect_steam_user_id(&steam_root);

    // Enumerate installed apps
    let mut installed_apps = HashMap::new();
    for lib in &lib_roots {
        let steamapps = lib.join("steamapps");
        if let Ok(entries) = fs::read_dir(&steamapps) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("acf") {
                    if let Some((appid, name, installdir)) = parse_acf_file(&path) {
                        let common = steamapps.join("common").join(&installdir);
                        if common.exists() {
                            installed_apps.insert(appid, (name, common, installdir));
                        }
                    }
                }
            }
        }
    }

    // Match manifest against installed apps
    for (title, game) in &manifest.games {
        let Some(steam_meta) = &game.steam else {
            continue;
        };
        let Some(appid) = steam_meta.id else {
            continue;
        };
        let Some((_name, install_location, install_dir_name)) = installed_apps.get(&appid) else {
            continue;
        };

        if results.contains_key(title) {
            continue;
        }

        // Ludusavi's `<root>` is the Steam root (`<root>/userdata/...`); `<base>`
        // is the game's own folder under `steamapps/common`.
        let root_str = steam_root.to_string_lossy().to_string();
        let base_str = install_location.to_string_lossy().to_string();
        let store_game_id_str = appid.to_string();

        let mut game_files = game.files.clone().unwrap_or_default();
        let ludusavi_yaml = install_location.join(".ludusavi.yaml");
        if ludusavi_yaml.exists() {
            if let Ok(file) = File::open(&ludusavi_yaml) {
                let reader = BufReader::new(file);
                if let Ok(sec) = serde_yaml_ng::from_reader::<_, SecondaryManifest>(reader) {
                    if let Some(files) = sec.files {
                        for (k, v) in files {
                            game_files.insert(k, v);
                        }
                    }
                }
            }
        }

        let mut verified_dirs = Vec::new();
        let mut candidate_dirs = Vec::new();
        for (template, details) in &game_files {
            if !is_save_path(&details.tags) {
                continue;
            }

            let resolved = resolve_placeholders(
                &template.replace("<base>", &base_str),
                Some(&root_str),
                Some(install_dir_name),
                store_user_id.as_deref(),
                Some(&store_game_id_str),
            );

            if let Some(candidate) = candidate_tracked_directory(&resolved) {
                let candidate = candidate.to_string_lossy().to_string();
                if !candidate_dirs.contains(&candidate) {
                    candidate_dirs.push(candidate);
                }
            }

            if let Some(dir) = resolve_tracked_directory(&resolved) {
                if dir.exists() {
                    let dir_str = dir.to_string_lossy().to_string();
                    if !verified_dirs.contains(&dir_str) {
                        verified_dirs.push(dir_str);
                    }
                }
            }
        }

        if !candidate_dirs.is_empty() {
            let mut icon_path = None;
            let steam_icon = steam_root
                .join("appcache")
                .join("librarycache")
                .join(format!("{}_icon.jpg", appid));
            if steam_icon.exists() {
                icon_path = Some(steam_icon.to_string_lossy().to_string());
            } else if let Some(ico) = find_ico_file(install_location) {
                icon_path = Some(ico);
            } else if let Some(exe) = find_largest_exe(install_location) {
                icon_path = Some(exe);
            }

            let last_modified_at = max_mtime_in_dirs(&verified_dirs);
            results.insert(
                title.clone(),
                InstallScannerItem {
                    title: title.clone(),
                    dirs: verified_dirs,
                    candidate_dirs,
                    install_path: Some(install_location.to_string_lossy().to_string()),
                    icon_path,
                    last_modified_at,
                    source: GameSource::Steam,
                },
            );
        }
    }
}

#[allow(clippy::collapsible_if)]
fn parse_epic_installs() -> Vec<EpicManifest> {
    #[allow(unused_mut)] // Only the Windows manifest scan adds entries.
    let mut installs = Vec::new();
    #[cfg(target_os = "windows")]
    {
        let manifests_dir = Path::new("C:\\ProgramData\\Epic\\EpicGamesLauncher\\Data\\Manifests");
        if manifests_dir.exists() {
            if let Ok(entries) = fs::read_dir(manifests_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|ext| ext.to_str()) == Some("item") {
                        if let Ok(file) = File::open(&path) {
                            let reader = BufReader::new(file);
                            if let Ok(manifest) = serde_json::from_reader::<_, EpicManifest>(reader)
                            {
                                installs.push(manifest);
                            }
                        }
                    }
                }
            }
        }
    }
    installs
}

#[allow(clippy::collapsible_if)]
fn resolve_epic_installs(manifest: &Manifest, results: &mut HashMap<String, InstallScannerItem>) {
    let epic_apps = parse_epic_installs();
    if epic_apps.is_empty() {
        return;
    }

    for app in epic_apps {
        let install_location = PathBuf::from(&app.install_location);
        if !install_location.exists() {
            continue;
        }

        // Match title
        let mut matched_title = None;
        let mut matched_game = None;

        if let Some(game) = manifest.games.get(&app.display_name) {
            matched_title = Some(&app.display_name);
            matched_game = Some(game);
        } else {
            let app_lower = app.display_name.to_lowercase();
            for (title, game) in &manifest.games {
                if title.to_lowercase() == app_lower {
                    matched_title = Some(title);
                    matched_game = Some(game);
                    break;
                }
            }
        }

        let Some(title) = matched_title else {
            continue;
        };
        let Some(game) = matched_game else {
            continue;
        };

        if results.contains_key(title) {
            continue;
        }

        let Some(parent) = install_location.parent() else {
            eprintln!("Epic install has no parent dir: {}", app.install_location);
            continue;
        };
        let root_str = parent.to_string_lossy().to_string();

        // Prefer the manifest's installDir name for <game> placeholder; fall back to actual folder name.
        let actual_folder = install_location
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        let game_folder_name = game
            .install_dir
            .as_ref()
            .and_then(|dirs| dirs.keys().next())
            .map(|s| s.as_str())
            .unwrap_or(actual_folder)
            .to_string();

        let mut game_files = game.files.clone().unwrap_or_default();
        let ludusavi_yaml = install_location.join(".ludusavi.yaml");
        if ludusavi_yaml.exists() {
            if let Ok(file) = File::open(&ludusavi_yaml) {
                let reader = BufReader::new(file);
                if let Ok(sec) = serde_yaml_ng::from_reader::<_, SecondaryManifest>(reader) {
                    if let Some(files) = sec.files {
                        for (k, v) in files {
                            game_files.insert(k, v);
                        }
                    }
                }
            }
        }

        let mut verified_dirs = Vec::new();
        let mut candidate_dirs = Vec::new();
        for (template, details) in &game_files {
            if !is_save_path(&details.tags) {
                continue;
            }

            let resolved = resolve_placeholders(
                template,
                Some(&root_str),
                Some(&game_folder_name),
                None,
                None,
            );

            if let Some(candidate) = candidate_tracked_directory(&resolved) {
                let candidate = candidate.to_string_lossy().to_string();
                if !candidate_dirs.contains(&candidate) {
                    candidate_dirs.push(candidate);
                }
            }

            if let Some(dir) = resolve_tracked_directory(&resolved) {
                if dir.exists() {
                    let dir_str = dir.to_string_lossy().to_string();
                    if !verified_dirs.contains(&dir_str) {
                        verified_dirs.push(dir_str);
                    }
                }
            }
        }

        if !candidate_dirs.is_empty() {
            let icon_path = app
                .launch_executable
                .as_ref()
                .and_then(|rel| {
                    let exe_path = install_location.join(rel);
                    if exe_path.exists() {
                        Some(exe_path.to_string_lossy().into_owned())
                    } else {
                        None
                    }
                })
                .or_else(|| find_ico_file(&install_location))
                .or_else(|| find_largest_exe(&install_location));

            let last_modified_at = max_mtime_in_dirs(&verified_dirs);
            results.insert(
                title.clone(),
                InstallScannerItem {
                    title: title.clone(),
                    dirs: verified_dirs,
                    candidate_dirs,
                    install_path: Some(app.install_location.clone()),
                    icon_path,
                    last_modified_at,
                    source: GameSource::Epic,
                },
            );
        }
    }
}

// ---------------------------------------------------------
// Pass 2 - OS Location Scan
// ---------------------------------------------------------

fn match_os_root_and_first_segment(template: &str) -> Option<(&'static str, String, String)> {
    let placeholders = [
        ("<winAppData>", "appdata"),
        ("<winLocalAppData>", "localappdata"),
        ("<winLocalAppDataLow>", "locallow"),
        ("<winDocuments>", "documents"),
    ];

    for (pl, root_name) in placeholders {
        if let Some(rest) = template.strip_prefix(pl) {
            let rest_trimmed = rest.trim_start_matches(['/', '\\']);
            let first_segment = rest_trimmed.split(['/', '\\']).next().unwrap_or("");
            if !first_segment.is_empty() {
                return Some((
                    root_name,
                    first_segment.to_string(),
                    rest_trimmed.to_string(),
                ));
            }
        }
    }
    None
}

fn is_glob_pattern(s: &str) -> bool {
    s.contains('*') || s.contains('?') || s.contains('[')
}

// Returns true when a glob wildcard appears inside a directory segment of the path
// (e.g. "Packages/*/LocalState"). These can't be resolved without full glob expansion
// and produce massive false-positive hits on common system directories.
fn has_dir_glob(template: &str) -> bool {
    if let Some(idx) = template.find(['*', '?', '[']) {
        let after = &template[idx + 1..];
        after.contains('/') || after.contains('\\')
    } else {
        false
    }
}

fn dir_has_entries(dir: &Path) -> bool {
    fs::read_dir(dir)
        .map(|mut e| e.next().is_some())
        .unwrap_or(false)
}

#[allow(clippy::collapsible_if)]
fn os_location_scan(manifest: &Manifest, results: &mut HashMap<String, InstallScannerItem>) {
    let home = dirs::home_dir().unwrap_or_default();
    let appdata_path = std::env::var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home.join("AppData/Roaming"));
    let localappdata_path = std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home.join("AppData/Local"));
    let locallow_path = home.join("AppData/LocalLow");
    let documents_path = dirs::document_dir().unwrap_or_else(|| home.join("Documents"));

    let appdata_subdirs = get_first_level_subdirs(&appdata_path);
    let localappdata_subdirs = get_first_level_subdirs(&localappdata_path);
    let locallow_subdirs = get_first_level_subdirs(&locallow_path);
    let documents_subdirs = get_first_level_subdirs(&documents_path);

    for (title, game) in &manifest.games {
        if results.contains_key(title) {
            continue;
        }

        let Some(files) = &game.files else {
            continue;
        };

        // Skip entries whose only file paths use `<base>`, `<root>`, or `<game>`
        let all_use_install_base = files
            .keys()
            .all(|k| k.contains("<base>") || k.contains("<root>") || k.contains("<game>"));
        if all_use_install_base {
            continue;
        }

        let mut verified_dirs = Vec::new();
        for (template, details) in files {
            if !is_save_path(&details.tags) {
                continue;
            }

            if template.contains("<base>")
                || template.contains("<root>")
                || template.contains("<game>")
            {
                continue;
            }

            // Skip "Packages/*/LocalState"-style paths — the wildcard is in a directory
            // segment, so the base dir (e.g. Packages/) exists on all Windows machines and
            // would produce false positives for every UWP game in the manifest.
            if has_dir_glob(template) {
                continue;
            }

            if let Some((root_name, first_segment, _rest)) =
                match_os_root_and_first_segment(template)
            {
                let subdirs = match root_name {
                    "appdata" => &appdata_subdirs,
                    "localappdata" => &localappdata_subdirs,
                    "locallow" => &locallow_subdirs,
                    "documents" => &documents_subdirs,
                    _ => continue,
                };

                if !is_glob_pattern(&first_segment)
                    && !subdirs.contains(&OsString::from(&first_segment))
                {
                    continue;
                }
            }

            let contains_user_id = template.contains("<storeUserId>");
            let resolved = if contains_user_id {
                let globbed_template = template.replace("<storeUserId>", "*");
                resolve_placeholders(&globbed_template, None, Some(title), None, None)
            } else {
                resolve_placeholders(template, None, Some(title), None, None)
            };

            if let Some(dir) = resolve_tracked_directory(&resolved) {
                if dir.exists() {
                    if contains_user_id {
                        if let Ok(entries) = fs::read_dir(&dir) {
                            let has_subdirs = entries.flatten().any(|entry| {
                                entry.file_type().map(|t| t.is_dir()).unwrap_or(false)
                            });
                            if has_subdirs {
                                let dir_str = dir.to_string_lossy().to_string();
                                if !verified_dirs.contains(&dir_str) {
                                    verified_dirs.push(dir_str);
                                }
                            }
                        }
                    } else if dir_has_entries(&dir) {
                        let dir_str = dir.to_string_lossy().to_string();
                        if !verified_dirs.contains(&dir_str) {
                            verified_dirs.push(dir_str);
                        }
                    }
                }
            }
        }

        if !verified_dirs.is_empty() {
            let last_modified_at = max_mtime_in_dirs(&verified_dirs);
            results.insert(
                title.clone(),
                InstallScannerItem {
                    title: title.clone(),
                    candidate_dirs: verified_dirs.clone(),
                    dirs: verified_dirs,
                    install_path: None,
                    icon_path: None,
                    last_modified_at,
                    source: GameSource::Local,
                },
            );
        }
    }
}

// ---------------------------------------------------------
// Pass 3 - Custom Roots Scan
// ---------------------------------------------------------

#[allow(clippy::collapsible_if)]
fn custom_roots_scan(
    manifest: &Manifest,
    custom_roots: &[String],
    results: &mut HashMap<String, InstallScannerItem>,
) {
    // Pre-build indexes so folder → game lookups are O(1) rather than O(n_manifest).
    let mut install_dir_index: HashMap<&str, &str> = HashMap::new();
    for (title, game) in &manifest.games {
        if let Some(dirs) = &game.install_dir {
            for dir_name in dirs.keys() {
                install_dir_index
                    .entry(dir_name.as_str())
                    .or_insert(title.as_str());
            }
        }
    }
    let title_lower_index: HashMap<String, &str> = manifest
        .games
        .keys()
        .map(|t| (t.to_lowercase(), t.as_str()))
        .collect();

    for root in custom_roots {
        let root_path = Path::new(root);
        if !root_path.exists() {
            continue;
        }

        if let Ok(entries) = fs::read_dir(root_path) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let folder_name = entry.file_name();
                    let folder_name_str = folder_name.to_string_lossy();

                    let matched_title: Option<&str> = install_dir_index
                        .get(folder_name_str.as_ref())
                        .copied()
                        .or_else(|| {
                            manifest
                                .games
                                .get_key_value(folder_name_str.as_ref())
                                .map(|(k, _)| k.as_str())
                        })
                        .or_else(|| {
                            title_lower_index
                                .get(&folder_name_str.to_lowercase())
                                .copied()
                        });

                    if let Some(title) = matched_title {
                        if let Some(game) = manifest.games.get(title) {
                            if results.contains_key(title) {
                                continue;
                            }

                            let root_str = root_path.to_string_lossy().to_string();
                            let install_dir_name = folder_name_str.to_string();

                            let mut game_files = game.files.clone().unwrap_or_default();
                            let ludusavi_yaml = path.join(".ludusavi.yaml");
                            if ludusavi_yaml.exists() {
                                if let Ok(file) = File::open(&ludusavi_yaml) {
                                    let reader = BufReader::new(file);
                                    if let Ok(sec) =
                                        serde_yaml_ng::from_reader::<_, SecondaryManifest>(reader)
                                    {
                                        if let Some(files) = sec.files {
                                            for (k, v) in files {
                                                game_files.insert(k, v);
                                            }
                                        }
                                    }
                                }
                            }

                            let mut verified_dirs = Vec::new();
                            for (template, details) in &game_files {
                                if !is_save_path(&details.tags) {
                                    continue;
                                }

                                let resolved = resolve_placeholders(
                                    template,
                                    Some(&root_str),
                                    Some(&install_dir_name),
                                    None,
                                    None,
                                );

                                if let Some(dir) = resolve_tracked_directory(&resolved) {
                                    if dir.exists() {
                                        let dir_str = dir.to_string_lossy().to_string();
                                        if !verified_dirs.contains(&dir_str) {
                                            verified_dirs.push(dir_str);
                                        }
                                    }
                                }
                            }

                            if !verified_dirs.is_empty() {
                                let icon_path =
                                    find_ico_file(&path).or_else(|| find_largest_exe(&path));
                                let last_modified_at = max_mtime_in_dirs(&verified_dirs);
                                results.insert(
                                    title.to_string(),
                                    InstallScannerItem {
                                        title: title.to_string(),
                                        candidate_dirs: verified_dirs.clone(),
                                        dirs: verified_dirs,
                                        install_path: Some(path.to_string_lossy().to_string()),
                                        icon_path,
                                        last_modified_at,
                                        source: GameSource::Custom,
                                    },
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

#[allow(clippy::collapsible_if)]
fn get_first_level_subdirs(path: &Path) -> HashSet<OsString> {
    let mut set = HashSet::new();
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            if let Ok(file_type) = entry.file_type() {
                if file_type.is_dir() {
                    set.insert(entry.file_name());
                }
            }
        }
    }
    set
}

// ---------------------------------------------------------
// Main scanner function
// ---------------------------------------------------------

pub fn scan_installed(
    cache_dir: &Path,
    custom_roots: &[String],
) -> Result<Vec<InstallScannerItem>> {
    Ok(scan_installed_candidates(cache_dir, custom_roots)?
        .into_iter()
        .filter(|item| !item.dirs.is_empty())
        .collect())
}

pub fn scan_installed_candidates(
    cache_dir: &Path,
    custom_roots: &[String],
) -> Result<Vec<InstallScannerItem>> {
    let manifest = load_manifest(cache_dir);
    let mut results = HashMap::new();

    // Pass 1 - Steam and Epic
    resolve_steam_installs(&manifest, &mut results);
    resolve_epic_installs(&manifest, &mut results);

    // Pass 2 - OS Location scan
    os_location_scan(&manifest, &mut results);

    // Pass 3 - Custom Roots
    custom_roots_scan(&manifest, custom_roots, &mut results);

    Ok(results.into_values().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn candidate_path_survives_before_the_save_directory_exists() {
        let save_dir = tempdir().unwrap().path().join("NeverRun");
        let candidate = candidate_tracked_directory(&save_dir.join("save.dat").to_string_lossy());
        assert_eq!(candidate, Some(save_dir));
    }

    #[test]
    fn test_resolve_tracked_directory_literal() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        // 1. Existing directory
        let res = resolve_tracked_directory(&path.to_string_lossy());
        assert_eq!(res, Some(path.to_path_buf()));

        // 2. Existing file
        let file_path = path.join("test.txt");
        fs::write(&file_path, "hello").unwrap();
        let res = resolve_tracked_directory(&file_path.to_string_lossy());
        assert_eq!(res, Some(path.to_path_buf()));

        // 3. Non-existing path
        let non_existing = path.join("does_not_exist");
        let res = resolve_tracked_directory(&non_existing.to_string_lossy());
        assert_eq!(res, None);
    }

    #[test]
    fn test_resolve_tracked_directory_glob() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        // Game specific dir with wildcard
        let game_dir = path.join("MyGame");

        // Scenario A: Game dir doesn't exist
        let pattern_no_exist = game_dir.join("*.sav");
        let res = resolve_tracked_directory(&pattern_no_exist.to_string_lossy());
        assert_eq!(res, None);

        // Scenario B: Game dir exists but has no saves yet
        fs::create_dir(&game_dir).unwrap();
        let res = resolve_tracked_directory(&pattern_no_exist.to_string_lossy());
        assert_eq!(res, Some(game_dir.clone())); // Game-specific dir exists, returns it directly

        // Scenario C: Wildcard prefix in shared directory (e.g. MyGames/SuperDXBall *)
        // Subdir doesn't exist, glob has no matches
        let shared_dir = path.join("My Games");
        fs::create_dir(&shared_dir).unwrap();
        let pattern_shared = shared_dir.join("SuperDXBall *");

        // We temporarily add shared_dir to be treated as shared in the test context
        // since path is a temp dir, it is not shared, but we can verify glob matching:
        // No match -> should return None even if parent exists
        let res = resolve_tracked_directory(&pattern_shared.to_string_lossy());
        assert_eq!(res, None);

        // Now create a matching directory
        let matched_dir = shared_dir.join("SuperDXBall Deluxe");
        fs::create_dir(&matched_dir).unwrap();
        let res = resolve_tracked_directory(&pattern_shared.to_string_lossy());
        assert_eq!(res, Some(matched_dir)); // The game's own folder, never the shared parent

        // Scenario D: a file pattern in an existing folder tracks that folder
        let saves = path.join("Saves");
        fs::create_dir(&saves).unwrap();
        fs::write(saves.join("slot1.dat"), "x").unwrap();
        let res = resolve_tracked_directory(&saves.join("slot*.dat").to_string_lossy());
        assert_eq!(res, Some(saves));
    }

    #[test]
    fn a_wildcard_folder_name_resolves_to_the_matched_folder() {
        // `<xdgConfig>/Vampire_Survivors_<storeUserId>/SaveData.sav` once
        // resolved to the whole of AppData\Roaming.
        let dir = tempdir().unwrap();
        let game = dir.path().join("Vampire_Survivors_123");
        fs::create_dir(&game).unwrap();
        fs::write(game.join("SaveData.sav"), "x").unwrap();
        let pattern = dir.path().join("Vampire_Survivors_*").join("SaveData.sav");
        assert_eq!(
            resolve_tracked_directory(&pattern.to_string_lossy()),
            Some(game)
        );
    }

    #[test]
    fn shared_folders_are_never_proposed() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(resolve_tracked_directory(&home.to_string_lossy()), None);
        let pattern = home.join("Game_*").join("SaveData.sav");
        assert_eq!(
            candidate_tracked_directory(&pattern.to_string_lossy()),
            None
        );
        let file = home.join("game.ini");
        assert_eq!(candidate_tracked_directory(&file.to_string_lossy()), None);
    }

    #[test]
    fn test_find_largest_exe() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        // No exes → None
        assert_eq!(find_largest_exe(path), None);

        // Single exe → that one
        fs::write(path.join("launcher.exe"), "x".repeat(100)).unwrap();
        assert_eq!(
            find_largest_exe(path),
            Some(path.join("launcher.exe").to_string_lossy().into_owned())
        );

        // Two exes → the larger one
        fs::write(path.join("game.exe"), "x".repeat(500)).unwrap();
        assert_eq!(
            find_largest_exe(path),
            Some(path.join("game.exe").to_string_lossy().into_owned())
        );

        // Non-exe files are ignored even if they are larger
        fs::write(path.join("readme.txt"), "x".repeat(9999)).unwrap();
        assert_eq!(
            find_largest_exe(path),
            Some(path.join("game.exe").to_string_lossy().into_owned())
        );

        // Known non-game exes are skipped even if largest
        fs::write(path.join("UnityCrashHandler64.exe"), "x".repeat(9000)).unwrap();
        fs::write(path.join("unins000.exe"), "x".repeat(8000)).unwrap();
        fs::write(path.join("CrashReportClient.exe"), "x".repeat(7000)).unwrap();
        assert_eq!(
            find_largest_exe(path),
            Some(path.join("game.exe").to_string_lossy().into_owned())
        );
    }
}

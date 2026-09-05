use std::borrow::Cow;
use std::path::{Path, PathBuf};

const PLACEHOLDER_HOME: &str = "<home>";
const PLACEHOLDER_OS_USER: &str = "<osUserName>";
const PLACEHOLDER_WIN_APPDATA: &str = "<winAppData>";
const PLACEHOLDER_WIN_LOCAL_APPDATA: &str = "<winLocalAppData>";
const PLACEHOLDER_WIN_LOCAL_APPDATA_LOW: &str = "<winLocalAppDataLow>";
const PLACEHOLDER_WIN_DOCUMENTS: &str = "<winDocuments>";
const PLACEHOLDER_WIN_PUBLIC: &str = "<winPublic>";
const PLACEHOLDER_WIN_PROGRAM_DATA: &str = "<winProgramData>";
const PLACEHOLDER_XDG_DATA: &str = "<xdgData>";
const PLACEHOLDER_XDG_CONFIG: &str = "<xdgConfig>";

pub fn expand_path_placeholders(path: &str) -> String {
    let mut expanded = path.to_string();

    for (placeholder, value) in environment_placeholders() {
        expanded = expanded.replace(placeholder, &value);
    }

    expanded.replace(
        PLACEHOLDER_OS_USER,
        &std::env::var("USERNAME")
            .or_else(|_| std::env::var("USER"))
            .unwrap_or_default(),
    )
}

pub fn substitute_path_placeholders(path: &Path) -> String {
    let raw_path = path_to_forward_slashes(path);
    let normalized_path = normalize_path_text_for_matching(&raw_path);

    for (placeholder, root) in prioritized_placeholder_roots() {
        let raw_root = path_to_forward_slashes(&root);
        let normalized_root = normalize_path_text_for_matching(&raw_root);

        if let Some(suffix) = strip_prefix_for_match(&raw_path, &normalized_path, &normalized_root)
        {
            if suffix.is_empty() {
                return placeholder.to_string();
            }

            return format!("{placeholder}/{suffix}");
        }
    }

    path_to_forward_slashes(path)
}

fn environment_placeholders() -> Vec<(&'static str, String)> {
    let mut pairs = Vec::new();

    if let Some(home) = dirs::home_dir() {
        pairs.push((PLACEHOLDER_HOME, path_to_forward_slashes(&home)));
        pairs.push((
            PLACEHOLDER_WIN_LOCAL_APPDATA_LOW,
            path_to_forward_slashes(&home.join("AppData").join("LocalLow")),
        ));
    }

    if let Some(docs) = dirs::document_dir() {
        pairs.push((PLACEHOLDER_WIN_DOCUMENTS, path_to_forward_slashes(&docs)));
    }

    if let Some(data) = dirs::data_dir() {
        pairs.push((PLACEHOLDER_XDG_DATA, path_to_forward_slashes(&data)));
    }

    if let Some(config) = dirs::config_dir() {
        pairs.push((PLACEHOLDER_XDG_CONFIG, path_to_forward_slashes(&config)));
    }

    if let Ok(appdata) = std::env::var("APPDATA") {
        pairs.push((PLACEHOLDER_WIN_APPDATA, appdata.replace('\\', "/")));
    }

    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        pairs.push((
            PLACEHOLDER_WIN_LOCAL_APPDATA,
            local_app_data.replace('\\', "/"),
        ));
    }

    if let Ok(public) = std::env::var("PUBLIC") {
        pairs.push((PLACEHOLDER_WIN_PUBLIC, public.replace('\\', "/")));
    }

    if let Ok(program_data) = std::env::var("PROGRAMDATA") {
        pairs.push((
            PLACEHOLDER_WIN_PROGRAM_DATA,
            program_data.replace('\\', "/"),
        ));
    }

    pairs
}

fn prioritized_placeholder_roots() -> Vec<(&'static str, PathBuf)> {
    let mut roots = Vec::new();

    if let Some(home) = dirs::home_dir() {
        roots.push((PLACEHOLDER_HOME, home.clone()));
        roots.push((
            PLACEHOLDER_WIN_LOCAL_APPDATA_LOW,
            home.join("AppData").join("LocalLow"),
        ));
    }

    if let Some(docs) = dirs::document_dir() {
        roots.push((PLACEHOLDER_WIN_DOCUMENTS, docs));
    }

    if let Ok(appdata) = std::env::var("APPDATA") {
        roots.push((PLACEHOLDER_WIN_APPDATA, PathBuf::from(appdata)));
    }

    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        roots.push((PLACEHOLDER_WIN_LOCAL_APPDATA, PathBuf::from(local_app_data)));
    }

    if let Some(data) = dirs::data_dir() {
        roots.push((PLACEHOLDER_XDG_DATA, data));
    }

    if let Some(config) = dirs::config_dir() {
        roots.push((PLACEHOLDER_XDG_CONFIG, config));
    }

    if let Ok(public) = std::env::var("PUBLIC") {
        roots.push((PLACEHOLDER_WIN_PUBLIC, PathBuf::from(public)));
    }

    if let Ok(program_data) = std::env::var("PROGRAMDATA") {
        roots.push((PLACEHOLDER_WIN_PROGRAM_DATA, PathBuf::from(program_data)));
    }

    roots
}

fn strip_prefix_for_match<'a>(
    raw_path: &'a str,
    normalized_path: &str,
    normalized_prefix: &str,
) -> Option<&'a str> {
    if normalized_path == normalized_prefix {
        return Some("");
    }

    let prefix_with_sep = format!("{normalized_prefix}/");
    if normalized_path.starts_with(&prefix_with_sep) {
        return raw_path.get(prefix_with_sep.len()..);
    }

    None
}

fn normalize_path_text_for_matching(path: &str) -> String {
    if cfg!(windows) {
        path.to_lowercase()
    } else {
        path.to_string()
    }
}

fn path_to_forward_slashes(path: &Path) -> String {
    let text: Cow<'_, str> = path.to_string_lossy();
    text.replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_known_placeholders() {
        let expanded = expand_path_placeholders("<home>/Saved Games/Test");
        assert!(!expanded.contains(PLACEHOLDER_HOME));
        assert!(expanded.ends_with("Saved Games/Test"));
    }

    #[test]
    fn prefers_home_for_user_profile_paths() {
        let home = dirs::home_dir().expect("home dir should exist for test");
        let target = home.join("Saved Games").join("Hollow Knight");
        let substituted = substitute_path_placeholders(&target);

        assert_eq!(substituted, "<home>/Saved Games/Hollow Knight");
    }

    #[test]
    fn keeps_verbatim_paths_when_no_placeholder_matches() {
        let path = Path::new("/definitely-not-a-known-root/game/saves");
        let substituted = substitute_path_placeholders(path);
        assert_eq!(substituted, "/definitely-not-a-known-root/game/saves");
    }
}

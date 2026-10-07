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

    let expanded = expanded.replace(
        PLACEHOLDER_OS_USER,
        &std::env::var("USERNAME")
            .or_else(|_| std::env::var("USER"))
            .unwrap_or_default(),
    );
    // Stored paths use `/`; hand the platform its native separator back.
    if cfg!(windows) {
        expanded.replace('/', "\\")
    } else {
        expanded
    }
}

pub fn substitute_path_placeholders(path: &Path) -> String {
    let raw_path = path_to_forward_slashes(path);

    // The longest matching root wins, so `<winDocuments>` beats `<home>` for
    // a path inside Documents (including OneDrive-redirected ones).
    // On ties the earlier (more specific) placeholder wins; `max_by_key`
    // keeps the last maximum, hence the reversal.
    let best = prioritized_placeholder_roots()
        .into_iter()
        .rev()
        .filter_map(|(placeholder, root)| {
            let (depth, suffix) = strip_root(&raw_path, &path_to_forward_slashes(&root))?;
            Some((depth, placeholder, suffix))
        })
        .max_by_key(|(depth, _, _)| *depth);
    if let Some((_, placeholder, suffix)) = best {
        if suffix.is_empty() {
            return placeholder.to_string();
        }
        return format!("{placeholder}/{suffix}");
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

/// When `path` lies inside `root`, the root's depth in components and the
/// rest of `path`. Components are compared one by one, so case folding that
/// changes a string's length never shifts the boundary.
fn strip_root(path: &str, root: &str) -> Option<(usize, String)> {
    let root = root.trim_end_matches('/');
    if root.is_empty() {
        return None;
    }
    let parts: Vec<&str> = path.split('/').collect();
    let root_parts: Vec<&str> = root.split('/').collect();
    let inside = parts.len() >= root_parts.len()
        && parts
            .iter()
            .zip(&root_parts)
            .all(|(part, root)| same_component(part, root));
    inside.then(|| (root_parts.len(), parts[root_parts.len()..].join("/")))
}

fn same_component(left: &str, right: &str) -> bool {
    if cfg!(windows) {
        left.to_lowercase() == right.to_lowercase()
    } else {
        left == right
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
        assert!(Path::new(&expanded).ends_with("Saved Games/Test"));
    }

    #[test]
    fn prefers_home_for_user_profile_paths() {
        let home = dirs::home_dir().expect("home dir should exist for test");
        let target = home.join("Saved Games").join("Hollow Knight");
        let substituted = substitute_path_placeholders(&target);

        assert_eq!(substituted, "<home>/Saved Games/Hollow Knight");
    }

    #[test]
    fn prefers_the_most_specific_root() {
        let Some(docs) = dirs::document_dir() else {
            return;
        };
        let substituted = substitute_path_placeholders(&docs.join("My Games").join("Save"));
        assert_eq!(substituted, "<winDocuments>/My Games/Save");
        assert!(Path::new(&expand_path_placeholders(&substituted)).ends_with("My Games/Save"));
    }

    #[test]
    fn case_folding_that_changes_length_keeps_the_suffix_intact() {
        // 'İ' lowercases to two chars, which used to cut the suffix short.
        let path = "C:/TestUnicodeİ/AppData/Saved/Game";
        assert_eq!(
            strip_root(path, "C:/TestUnicodeİ/AppData/"),
            Some((3, "Saved/Game".to_string()))
        );
        assert_eq!(strip_root(path, "C:/TestUnicodeİ/App"), None);
        if cfg!(windows) {
            assert_eq!(
                strip_root(path, "c:/testunicodei\u{307}/appdata"),
                Some((3, "Saved/Game".to_string()))
            );
        }
    }

    #[test]
    fn keeps_verbatim_paths_when_no_placeholder_matches() {
        let path = Path::new("/definitely-not-a-known-root/game/saves");
        let substituted = substitute_path_placeholders(path);
        assert_eq!(substituted, "/definitely-not-a-known-root/game/saves");
    }
}

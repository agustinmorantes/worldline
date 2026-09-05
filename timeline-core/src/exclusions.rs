use glob::{MatchOptions, Pattern};
use std::path::Path;

/// Matching is case-insensitive: the global exclusion list in `fs_utils` already
/// lowercases, and the primary platform has case-insensitive filesystems, so a
/// pattern that silently stopped matching on `SAVE.CFG` would be a data-loss trap.
const MATCH_OPTIONS: MatchOptions = MatchOptions {
    case_sensitive: false,
    require_literal_separator: false,
    require_literal_leading_dot: false,
};

/// Parses a tracked dir's `exclusions` column (a JSON array of glob patterns).
/// Invalid JSON or invalid patterns yield no exclusions rather than an error:
/// a malformed exclusion must never cause a restore to fail.
pub fn parse(raw: &str) -> Vec<Pattern> {
    serde_json::from_str::<Vec<String>>(raw)
        .unwrap_or_default()
        .iter()
        .filter_map(|pattern| Pattern::new(pattern).ok())
        .collect()
}

/// True if `relative_path` (relative to the tracked dir root) matches any pattern,
/// as a whole path or by file name alone. The file-name fallback is what makes a
/// literal, wildcard-free pattern such as `quicksave.dat` match at any depth; patterns
/// containing `*` already span separators, because `require_literal_separator` is off.
pub fn is_excluded(patterns: &[Pattern], relative_path: &Path) -> bool {
    if patterns.is_empty() {
        return false;
    }
    let as_str = relative_path.to_string_lossy().replace('\\', "/");
    let file_name = relative_path.file_name().map(|n| n.to_string_lossy());
    patterns.iter().any(|pattern| {
        pattern.matches_with(&as_str, MATCH_OPTIONS)
            || file_name
                .as_ref()
                .is_some_and(|name| pattern.matches_with(name, MATCH_OPTIONS))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_by_extension_and_by_full_relative_path() {
        let patterns = parse(r#"["*.cfg", "mods/**"]"#);
        assert!(is_excluded(&patterns, Path::new("settings.cfg")));
        assert!(is_excluded(&patterns, Path::new("nested/settings.cfg")));
        assert!(is_excluded(&patterns, Path::new("mods/cool/thing.dll")));
        assert!(!is_excluded(&patterns, Path::new("save.dat")));
    }

    #[test]
    fn matching_is_case_insensitive_and_separator_agnostic() {
        let patterns = parse(r#"["*.cfg", "mods/**"]"#);
        assert!(is_excluded(&patterns, Path::new("SETTINGS.CFG")));
        assert!(is_excluded(&patterns, Path::new("mods\\cool\\thing.dll")));
    }

    #[test]
    fn malformed_exclusions_exclude_nothing() {
        assert!(parse("not json").is_empty());
        assert!(!is_excluded(&parse("not json"), Path::new("save.dat")));
        assert!(parse(r#"["["]"#).is_empty(), "invalid globs are dropped");
    }
}

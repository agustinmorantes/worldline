//! Puts the folder holding this executable on the user's PATH, or takes it off.
//! The Windows installer runs these right after copying the files and right before removing them.

use std::io;
use std::path::PathBuf;

use anyhow::{Context, Result};
use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, RegType};
use winreg::types::{FromRegValue, ToRegValue};

pub fn add() -> Result<()> {
    update(with_entry)
}

pub fn remove() -> Result<()> {
    update(without_entry)
}

fn update(change: fn(&str, &str) -> Option<String>) -> Result<()> {
    let exe = std::env::current_exe().context("failed to find this executable")?;
    let dir = exe
        .parent()
        .map(PathBuf::from)
        .context("this executable has no folder")?;
    let env = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags("Environment", KEY_READ | KEY_WRITE)
        .context("failed to open the user environment")?;
    let (current, vtype) = match env.get_raw_value("Path") {
        Ok(value) => (String::from_reg_value(&value)?, value.vtype),
        // PATH usually holds %VARIABLES%, so a new one is expandable too.
        Err(e) if e.kind() == io::ErrorKind::NotFound => (String::new(), RegType::REG_EXPAND_SZ),
        Err(e) => return Err(e).context("failed to read the user PATH"),
    };
    if let Some(changed) = change(&current, &dir.to_string_lossy()) {
        let mut value = changed.to_reg_value();
        value.vtype = vtype;
        env.set_raw_value("Path", &value)
            .context("failed to write the user PATH")?;
    }
    Ok(())
}

fn is_dir(entry: &str, dir: &str) -> bool {
    entry.trim_end_matches('\\').to_lowercase() == dir.trim_end_matches('\\').to_lowercase()
}

/// The PATH with `dir` appended, or `None` when it's already there.
fn with_entry(path: &str, dir: &str) -> Option<String> {
    if path.split(';').any(|entry| is_dir(entry, dir)) {
        return None;
    }
    Some(match path {
        "" => dir.to_string(),
        // Keeping the trailing separator lets removing restore the PATH exactly.
        _ if path.ends_with(';') => format!("{path}{dir};"),
        _ => format!("{path};{dir}"),
    })
}

/// The PATH without `dir`, or `None` when it isn't there.
fn without_entry(path: &str, dir: &str) -> Option<String> {
    if !path.split(';').any(|entry| is_dir(entry, dir)) {
        return None;
    }
    Some(
        path.split(';')
            .filter(|entry| !is_dir(entry, dir))
            .collect::<Vec<_>>()
            .join(";"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIR: &str = r"C:\Users\Agustín\AppData\Local\Worldline";

    #[test]
    fn adds_to_the_end() {
        assert_eq!(
            with_entry(r"C:\a;C:\b", DIR),
            Some(format!(r"C:\a;C:\b;{DIR}"))
        );
        assert_eq!(with_entry(r"C:\a;", DIR), Some(format!(r"C:\a;{DIR};")));
        assert_eq!(with_entry("", DIR), Some(DIR.to_string()));
    }

    #[test]
    fn removing_restores_what_adding_changed() {
        for path in ["", r"C:\a", r"C:\a;", r"%USERPROFILE%\bin;;C:\b;"] {
            let added = with_entry(path, DIR).unwrap();
            assert_eq!(without_entry(&added, DIR).as_deref(), Some(path));
        }
    }

    #[test]
    fn adding_twice_changes_nothing() {
        let upper = format!(r"C:\a;{}\", DIR.to_uppercase());
        assert_eq!(with_entry(&upper, DIR), None);
    }

    #[test]
    fn removes_only_its_own_entry() {
        let path = format!(r"%USERPROFILE%\bin;{DIR};;C:\b;{DIR}\");
        assert_eq!(
            without_entry(&path, DIR),
            Some(r"%USERPROFILE%\bin;;C:\b".to_string())
        );
        assert_eq!(without_entry(r"C:\a;C:\b", DIR), None);
    }
}

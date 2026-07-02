use std::path::{Path, PathBuf};

/// Resolve the Codex.exe to launch.
///
/// When `use_junction` is true, scan for the newest numeric-version dir,
/// verify the junction points at it, and return the junction path
/// (`versions/current/Codex.exe`) when available. Launching via the stable
/// junction path lets user-applied AV exclusions survive updates.
///
/// When `use_junction` is false, or the junction can't be established,
/// return the newest numeric-version `Codex.exe` directly.
pub fn latest_codex_exe(root: &Path, use_junction: bool) -> Option<PathBuf> {
    let versions = root.join("versions");
    let (newest_name, newest_exe) = newest_numeric_version(&versions)?;

    if !use_junction {
        return Some(newest_exe);
    }

    let link = versions.join("current");
    let expected_target = versions.join(&newest_name);

    let stale = match std::fs::canonicalize(&link) {
        Ok(actual) => std::fs::canonicalize(&expected_target)
            .map(|want| actual != want)
            .unwrap_or(true),
        Err(_) => true,
    };

    if stale {
        if let Err(e) = crate::junction::set_current(root, &newest_name) {
            eprintln!("warn: couldn't repair versions/current junction: {e:#}");
            return Some(newest_exe);
        }
    }

    let via_junction = link.join("Codex.exe");
    if via_junction.exists() {
        Some(via_junction)
    } else {
        Some(newest_exe)
    }
}

fn newest_numeric_version(versions: &Path) -> Option<(String, PathBuf)> {
    let mut best: Option<(Vec<u64>, String, PathBuf)> = None;
    for entry in std::fs::read_dir(versions).ok()? {
        let entry = entry.ok()?;
        if !entry.path().is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".partial") || name == "current" {
            continue;
        }
        if entry.file_type().map(|t| t.is_symlink()).unwrap_or(false) {
            continue;
        }
        let parts: Vec<u64> = name.split('.').map(|p| p.parse().unwrap_or(0)).collect();
        let codex = entry.path().join("Codex.exe");
        if !codex.exists() {
            continue;
        }
        match &best {
            None => best = Some((parts, name, codex)),
            Some((cur, _, _)) if parts > *cur => best = Some((parts, name, codex)),
            _ => {}
        }
    }
    best.map(|(_, name, codex)| (name, codex))
}

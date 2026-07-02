use crate::config::{Config, CONFIG_FILENAME, LEGACY_CONFIG_FILENAME};
use std::path::{Path, PathBuf};

pub enum Mode {
    Installer,
    Proxy(Config),
}

/// The launcher's behavior is determined by whether a valid `updater.json`
/// lives next to the executable. No file → installer mode. Valid file →
/// proxy mode. Corrupt file also falls back to installer so the user has
/// a recovery path.
///
/// Runs a one-shot migration: if `updater.json` is absent but the legacy
/// `config.json` is present and parses as our schema, rename it. If the
/// legacy file is present but doesn't parse, we leave it alone — it
/// almost certainly belongs to Codex (Electron apps write a `config.json`
/// to their userData dir) and was never ours.
pub fn detect() -> anyhow::Result<Mode> {
    let dir = install_root()?;
    migrate_legacy_config(&dir);

    let path = dir.join(CONFIG_FILENAME);
    if !path.exists() {
        return Ok(Mode::Installer);
    }
    // Use load_runtime so System installs (whose install-root config may
    // be frozen at install time) still pick up runtime state from the
    // per-user fallback file when present.
    match Config::load_runtime(&dir) {
        Ok(cfg) => Ok(Mode::Proxy(cfg)),
        Err(_) => Ok(Mode::Installer),
    }
}

/// Best-effort migration from `config.json` → `updater.json`. Silent on
/// failure — the worst case is the user sees installer mode on next run
/// and can reinstall.
fn migrate_legacy_config(dir: &Path) {
    let new_path = dir.join(CONFIG_FILENAME);
    let old_path = dir.join(LEGACY_CONFIG_FILENAME);
    if new_path.exists() || !old_path.exists() {
        return;
    }
    // Only migrate if the file parses as our schema — otherwise it's
    // not ours to touch (likely Codex's own config).
    if Config::load(&old_path).is_ok() {
        let _ = std::fs::rename(&old_path, &new_path);
    }
}

pub fn install_root() -> anyhow::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let dir = exe
        .parent()
        .ok_or_else(|| anyhow::anyhow!("executable has no parent directory"))?;
    Ok(dir.to_path_buf())
}

//! MSIX extraction + version directory management.
//!
//! MSIX is a ZIP archive. The Electron app we care about lives under the
//! `app/` prefix; everything else (AppxManifest.xml, AppxBlockMap.xml,
//! AppxSignature.p7x, Assets/, resources.pri, ...) is Store packaging
//! metadata we don't need to run Codex standalone.
//!
//! Layout produced:
//!   <install_root>/versions/<version>/Codex.exe
//!   <install_root>/versions/<version>/resources/app.asar
//!   ...
//!
//! Extraction writes to `<version>.partial/` first and renames on success,
//! so a crash mid-extract never leaves a half-populated directory that
//! looks valid to the proxy launcher.

use anyhow::{bail, Context, Result};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const APP_PREFIX: &str = "app/";

/// Extract the `app/` subtree from `msix_path` into
/// `<install_root>/versions/<version>/`. Any pre-existing directory at that
/// path is removed first. Progress fires per-entry with (done_entries, total_entries).
pub fn extract_app(
    msix_path: &Path,
    install_root: &Path,
    version: &str,
    progress: &mut dyn FnMut(u64, Option<u64>) -> Result<()>,
) -> Result<PathBuf> {
    let versions_dir = install_root.join("versions");
    fs::create_dir_all(&versions_dir)
        .with_context(|| format!("creating {}", versions_dir.display()))?;

    let final_dir = versions_dir.join(version);
    let partial_dir = versions_dir.join(format!("{version}.partial"));

    if partial_dir.exists() {
        fs::remove_dir_all(&partial_dir)
            .with_context(|| format!("clearing stale {}", partial_dir.display()))?;
    }
    fs::create_dir_all(&partial_dir)?;

    let file =
        fs::File::open(msix_path).with_context(|| format!("opening {}", msix_path.display()))?;
    let mut zip = zip::ZipArchive::new(file)
        .with_context(|| format!("reading {} as zip", msix_path.display()))?;

    // First pass: count app/ entries so progress has a total.
    let mut total_app_entries: u64 = 0;
    for i in 0..zip.len() {
        let entry = zip.by_index(i)?;
        if entry.name().starts_with(APP_PREFIX) && !entry.is_dir() {
            total_app_entries += 1;
        }
    }
    if total_app_entries == 0 {
        bail!(
            "MSIX contains no entries under '{}' — wrong package? {}",
            APP_PREFIX,
            msix_path.display()
        );
    }

    let mut done: u64 = 0;
    progress(done, Some(total_app_entries))?;

    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let name = entry.name().to_string();
        if !name.starts_with(APP_PREFIX) {
            continue;
        }
        let rel = &name[APP_PREFIX.len()..];
        if rel.is_empty() {
            continue;
        }
        let out_path = safe_join(&partial_dir, rel)?;

        if entry.is_dir() {
            fs::create_dir_all(&out_path)?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out = fs::File::create(&out_path)
            .with_context(|| format!("creating {}", out_path.display()))?;
        io::copy(&mut entry, &mut out)?;

        done += 1;
        progress(done, Some(total_app_entries))?;
    }

    if final_dir.exists() {
        fs::remove_dir_all(&final_dir)
            .with_context(|| format!("removing old {}", final_dir.display()))?;
    }
    rename_with_retry(&partial_dir, &final_dir).with_context(|| {
        format!(
            "rename {} -> {}",
            partial_dir.display(),
            final_dir.display()
        )
    })?;

    let codex_exe = final_dir.join("Codex.exe");
    if !codex_exe.exists() {
        bail!(
            "extracted tree has no Codex.exe at {} — MSIX layout changed?",
            codex_exe.display()
        );
    }

    Ok(final_dir)
}

/// Reject absolute paths, drive letters, and `..` traversal. ZIP entries
/// are untrusted input and the Store MSIX is signed but we still validate.
fn safe_join(base: &Path, rel: &str) -> Result<PathBuf> {
    let rel_path = Path::new(rel);
    if rel_path.is_absolute() {
        bail!("zip entry has absolute path: {}", rel);
    }
    let mut out = base.to_path_buf();
    for comp in rel_path.components() {
        use std::path::Component::*;
        match comp {
            Normal(c) => out.push(c),
            CurDir => {}
            ParentDir => bail!("zip entry escapes base via '..': {}", rel),
            Prefix(_) | RootDir => bail!("zip entry has root/prefix component: {}", rel),
        }
    }
    Ok(out)
}

/// Keep the `keep` newest versions (by semver-ish numeric sort of directory
/// names) under `<install_root>/versions/`, delete the rest. Also cleans up
/// stale `*.partial` directories. Returns the list of removed directory names.
pub fn prune_versions(install_root: &Path, keep: u32) -> Result<Vec<String>> {
    let versions_dir = install_root.join("versions");
    if !versions_dir.exists() {
        return Ok(Vec::new());
    }

    let mut versions: Vec<(Vec<u64>, String, PathBuf)> = Vec::new();
    let mut removed: Vec<String> = Vec::new();

    for entry in fs::read_dir(&versions_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = match entry.file_name().to_str() {
            Some(s) => s.to_string(),
            None => continue,
        };
        // Skip reparse points (e.g. the `versions/current` junction).
        // `remove_dir_all` on a junction would recurse into the target.
        if entry.file_type().map(|t| t.is_symlink()).unwrap_or(false) {
            continue;
        }
        if name.ends_with(".partial") {
            let _ = fs::remove_dir_all(&path);
            removed.push(name);
            continue;
        }
        let parts = parse_version(&name);
        versions.push((parts, name, path));
    }

    // Sort descending — newest first.
    versions.sort_by(|a, b| b.0.cmp(&a.0));

    let keep = keep.max(1) as usize;
    for (_, name, path) in versions.into_iter().skip(keep) {
        if let Err(e) = fs::remove_dir_all(&path) {
            eprintln!("warn: failed to remove {}: {}", path.display(), e);
            continue;
        }
        removed.push(name);
    }
    Ok(removed)
}

/// Windows AV (Defender) commonly holds transient handles on freshly-written
/// executables, causing `rename` of a directory tree to fail with ACCESS_DENIED.
/// Retry a few times with backoff before giving up.
fn rename_with_retry(from: &Path, to: &Path) -> io::Result<()> {
    let mut delay_ms = 100u64;
    for attempt in 0..6 {
        match fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) if attempt < 5 => {
                std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                delay_ms *= 2;
                let _ = e;
            }
            Err(e) => return Err(e),
        }
    }
    unreachable!()
}

fn parse_version(s: &str) -> Vec<u64> {
    s.split('.')
        .map(|p| p.parse::<u64>().unwrap_or(0))
        .collect()
}

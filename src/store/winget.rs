//! Winget fetcher: shells out to the Windows Package Manager CLI.
//!
//! Used as a fallback when the direct FE3 path is unreachable (firewall,
//! DPI, cert pinning, etc.) or when the user explicitly prefers it. Requires
//! `winget.exe` on PATH; Windows 11 has it by default, modern Windows 10 as
//! well through the "App Installer" Store package.
//!
//! No granular download progress — winget doesn't expose a structured
//! progress stream. We call `progress(0, None)` at start and report the
//! final file size on completion.

use super::DownloadResult;
use anyhow::{anyhow, bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

pub(super) fn download_latest(
    product_id: &str,
    dest_dir: &Path,
    progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<DownloadResult> {
    ensure_winget_exists()?;
    std::fs::create_dir_all(dest_dir)?;

    // Signal start — UI can show an indeterminate state.
    progress(0, None);

    // Drop any stale .msix/.msixbundle from a previous invocation so
    // find_downloaded_msix below unambiguously picks the new one.
    clear_existing_msix(dest_dir);

    let output = Command::new("winget")
        .args([
            "download",
            "--id",
            product_id,
            "--source",
            "msstore",
            "--accept-source-agreements",
            "--accept-package-agreements",
            "--architecture",
            "x64",
            "--skip-dependencies",
            "--download-directory",
        ])
        .arg(dest_dir)
        .output()
        .context("failed to invoke winget (is it installed and on PATH?)")?;

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "winget download failed (exit {:?})\n--- stdout ---\n{}\n--- stderr ---\n{}",
            output.status.code(),
            stdout.trim(),
            stderr.trim()
        );
    }

    let (msix_path, moniker) = find_downloaded_msix(dest_dir)?;
    let version = moniker
        .split('_')
        .nth(1)
        .ok_or_else(|| anyhow!("could not parse version from moniker: {moniker}"))?
        .to_string();

    // Report final size so the UI can reflect "done".
    if let Ok(meta) = std::fs::metadata(&msix_path) {
        let n = meta.len();
        progress(n, Some(n));
    }

    Ok(DownloadResult {
        msix_path,
        moniker,
        version,
    })
}

fn ensure_winget_exists() -> Result<()> {
    Command::new("winget")
        .arg("--version")
        .output()
        .context("winget is not available on this system")
        .and_then(|o| {
            if o.status.success() {
                Ok(())
            } else {
                bail!("winget exited non-zero on --version check")
            }
        })
}

fn clear_existing_msix(dir: &Path) {
    let Ok(iter) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in iter.flatten() {
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.ends_with(".msix") || name.ends_with(".msixbundle") {
            let _ = std::fs::remove_file(&path);
        }
    }
}

fn find_downloaded_msix(dir: &Path) -> Result<(PathBuf, String)> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.ends_with(".msix") || name.ends_with(".msixbundle") {
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            return Ok((path, stem));
        }
    }
    bail!("no .msix/.msixbundle file found in {}", dir.display());
}

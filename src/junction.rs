//! `versions/current` directory junction — stable path that always points
//! at the newest installed version.
//!
//! Uses `mklink /J` under the hood (directory junction / mount point reparse
//! point). No admin required, no Developer Mode required, same-volume only
//! — which fits our case since `current` lives in the same `versions/` dir
//! as the targets.
//!
//! Removal uses `fs::remove_dir`, which on a junction removes the link and
//! leaves the target intact (NTFS treats it as an empty directory for the
//! purposes of the delete).

use anyhow::{Context, Result};
use std::path::Path;

/// Point `<root>/versions/current` at `<root>/versions/<version>/`. If a
/// junction (or anything else) already exists at `current`, it is removed
/// first. Errors are non-fatal at the caller — the install is still usable
/// without the junction.
pub fn set_current(root: &Path, version: &str) -> Result<()> {
    let link = root.join("versions").join("current");
    let target = root.join("versions").join(version);

    if !target.is_dir() {
        anyhow::bail!("junction target {} does not exist", target.display());
    }

    let _ = remove(&link); // best effort; proceed even if it wasn't there

    #[cfg(windows)]
    {
        let status = std::process::Command::new("cmd")
            .args([
                "/c",
                "mklink",
                "/J",
                &link.to_string_lossy(),
                &target.to_string_lossy(),
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .with_context(|| format!("spawning mklink for {}", link.display()))?;
        if !status.success() {
            anyhow::bail!("mklink /J failed with exit code {:?}", status.code());
        }
        Ok(())
    }

    #[cfg(not(windows))]
    {
        let _ = (link, target);
        anyhow::bail!("junctions only supported on Windows");
    }
}

/// Remove `<root>/versions/current` if present. Safe to call when nothing
/// is there. On Windows, `fs::remove_dir` on a junction unlinks it without
/// touching the target.
pub fn remove(link: &Path) -> Result<()> {
    if !link.exists() && !is_reparse_point(link) {
        return Ok(());
    }
    std::fs::remove_dir(link).with_context(|| format!("removing junction at {}", link.display()))
}

/// True if `path` is a reparse point (junction or symlink). `exists()` can
/// return false for dangling junctions, so we check metadata directly.
fn is_reparse_point(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
}

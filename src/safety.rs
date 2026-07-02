//! Blast-radius guardrails for destructive operations (currently: uninstall).
//!
//! The uninstaller used to take `exe.parent()` verbatim and hand it to
//! `rmdir /s /q` — meaning if the launcher was ever placed in the wrong
//! directory (Desktop, Documents, Downloads), those got wiped. This module
//! refuses to proceed unless `root` matches a known install-shape AND isn't
//! a user-profile / system directory.
//!
//! See POSTMORTEM_phase7_uninstaller.md for the incident that motivated this.

use crate::config::CONFIG_FILENAME;
use anyhow::{bail, Result};
use std::path::{Path, PathBuf};
use windows::core::GUID;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{
    FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_LocalAppData,
    FOLDERID_Music, FOLDERID_Pictures, FOLDERID_Profile, FOLDERID_ProgramFiles,
    FOLDERID_ProgramFilesX86, FOLDERID_Public, FOLDERID_RoamingAppData, FOLDERID_Videos,
    FOLDERID_Windows, SHGetKnownFolderPath, KF_FLAG_DEFAULT,
};

/// Validate that `root` looks like a legitimate install root and is safe to
/// wipe. Returns `Err` with a human-readable reason if not.
///
/// Required signature (all must hold):
/// - `root/updater.json` exists and is a file
/// - `root/versions/` exists and is a directory
/// - `root/codex-launcher.exe` exists and is a file
/// - `root` canonicalized is not equal to any Known Folder (user profile,
///   Desktop, Documents, Downloads, AppData, Program Files, Windows, etc.)
/// - `root` is not a drive root (e.g. `C:\`)
pub fn validate_uninstall_root(root: &Path) -> Result<()> {
    let canon = std::fs::canonicalize(root)
        .map_err(|e| anyhow::anyhow!("canonicalize {}: {e}", root.display()))?;

    // Structural signature — all three must be present. If any are missing,
    // this isn't our install and we shouldn't touch anything here.
    if !root.join(CONFIG_FILENAME).is_file() {
        bail!(
            "refusing to uninstall: {} missing {}",
            root.display(),
            CONFIG_FILENAME
        );
    }
    if !root.join("versions").is_dir() {
        bail!(
            "refusing to uninstall: {} missing versions/ subdir",
            root.display()
        );
    }
    if !root.join("codex-launcher.exe").is_file() {
        bail!(
            "refusing to uninstall: {} missing codex-launcher.exe",
            root.display()
        );
    }

    // Drive-root check — path that canonicalizes to "C:\" etc.
    let s = canon.to_string_lossy();
    if s.len() <= 3 && s.ends_with('\\') {
        bail!("refusing to uninstall a drive root: {}", canon.display());
    }

    // Known-folder blacklist. If the launcher was dropped directly into a
    // user profile dir, we must not nuke that dir.
    for (label, guid) in forbidden_folders() {
        // If we can't resolve the known folder, skip silently — one fewer
        // guardrail, but never a hard fail on this path.
        if let Some(kf) = known_folder(&guid) {
            if paths_equal(&canon, &kf) {
                bail!(
                    "refusing to uninstall: {} is the {} folder",
                    canon.display(),
                    label
                );
            }
        }
    }

    Ok(())
}

fn forbidden_folders() -> Vec<(&'static str, GUID)> {
    vec![
        ("user profile", FOLDERID_Profile),
        ("Desktop", FOLDERID_Desktop),
        ("Documents", FOLDERID_Documents),
        ("Downloads", FOLDERID_Downloads),
        ("Pictures", FOLDERID_Pictures),
        ("Music", FOLDERID_Music),
        ("Videos", FOLDERID_Videos),
        ("Public", FOLDERID_Public),
        ("AppData\\Local", FOLDERID_LocalAppData),
        ("AppData\\Roaming", FOLDERID_RoamingAppData),
        ("Windows", FOLDERID_Windows),
        ("Program Files", FOLDERID_ProgramFiles),
        ("Program Files (x86)", FOLDERID_ProgramFilesX86),
    ]
}

fn known_folder(id: &GUID) -> Option<PathBuf> {
    unsafe {
        let pw = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let mut len = 0usize;
        while *pw.0.add(len) != 0 {
            len += 1;
        }
        let slice = std::slice::from_raw_parts(pw.0, len);
        let s = String::from_utf16_lossy(slice);
        CoTaskMemFree(Some(pw.0 as *mut _));
        let p = PathBuf::from(s);
        std::fs::canonicalize(&p).ok().or(Some(p))
    }
}

fn paths_equal(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        p.to_string_lossy()
            .trim_end_matches('\\')
            .to_ascii_lowercase()
    };
    norm(a) == norm(b)
}

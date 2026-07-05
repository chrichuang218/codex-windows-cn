//! Deletion primitives for uninstall / update cleanup.
//!
//! Three things this module provides:
//!
//! 1. `retry_delete_file` / `retry_delete_dir_all` — bounded-retry delete that
//!    absorbs transient sharing violations (AV scan, thumbnail gen, indexer)
//!    but gives up quickly when a lock is persistent (Explorer window,
//!    another app actually using a file). Hard budget per op so we never
//!    spin like `rmdir /s /q` did.
//!
//! 2. `delete_self_exe` — tiered self-delete of the running launcher.
//!    Tier (a): POSIX semantics (Win10 1809+, NTFS) via
//!    `SetFileInformationByHandle` with `FileDispositionInfoEx` +
//!    `FILE_DISPOSITION_POSIX_SEMANTICS`. Unlinks immediately while we
//!    still execute; data vanishes when the loader's handle closes on
//!    process exit. No admin, no reboot.
//!    Tier (b): `MoveFileExW(MOVEFILE_DELAY_UNTIL_REBOOT)` — needs admin
//!    (writes HKLM's PendingFileRenameOperations). smss.exe processes the
//!    queue at next boot.
//!    Tier (c): give up — return `LeftBehind`; caller logs it.
//!
//! 3. `CleanupReport` — per-path deleted/skipped record, plus the
//!    self-delete outcome. Caller writes this to a log file.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const RETRY_BUDGET: Duration = Duration::from_millis(2500);
const RETRY_STEPS_MS: &[u64] = &[50, 100, 250, 500, 1000];

/// ERROR_SHARING_VIOLATION. File is open by another process without FILE_SHARE_DELETE.
const ERROR_SHARING_VIOLATION: i32 = 32;
/// ERROR_ACCESS_DENIED. Often raised for the same reason as above in practice.
const ERROR_ACCESS_DENIED: i32 = 5;
/// ERROR_LOCK_VIOLATION. Region of file is locked.
const ERROR_LOCK_VIOLATION: i32 = 33;
/// ERROR_DIR_NOT_EMPTY. Expected during intermediate rmdir passes.
const ERROR_DIR_NOT_EMPTY: i32 = 145;

#[derive(Debug)]
pub struct CleanupReport {
    pub deleted: Vec<PathBuf>,
    pub skipped: Vec<(PathBuf, String)>,
    pub self_delete: SelfDeleteOutcome,
}

#[derive(Debug)]
pub enum SelfDeleteOutcome {
    /// POSIX unlink succeeded — file is already gone, handle still valid until exit.
    PosixUnlinked,
    /// Legacy value kept so old uninstall logs remain understandable.
    SpawnedCleanup,
    /// Scheduled for delete at next boot via PendingFileRenameOperations.
    ScheduledForReboot,
    /// Couldn't delete by any mechanism. Exe will linger; user must clean manually.
    LeftBehind(String),
}

impl std::fmt::Display for SelfDeleteOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SelfDeleteOutcome::PosixUnlinked => f.write_str("PosixUnlinked"),
            SelfDeleteOutcome::SpawnedCleanup => f.write_str("SpawnedCleanup"),
            SelfDeleteOutcome::ScheduledForReboot => f.write_str("ScheduledForReboot"),
            SelfDeleteOutcome::LeftBehind(reason) => write!(f, "LeftBehind ({reason})"),
        }
    }
}

impl CleanupReport {
    pub fn new() -> Self {
        Self {
            deleted: Vec::new(),
            skipped: Vec::new(),
            self_delete: SelfDeleteOutcome::LeftBehind("not attempted".into()),
        }
    }

    pub fn to_log_string(&self, root: &Path) -> String {
        use std::fmt::Write as _;
        let mut out = String::new();
        out.push_str("Codex uninstaller cleanup report\n");
        let _ = writeln!(out, "Install root: {}", root.display());
        let _ = writeln!(out, "Deleted: {} entries", self.deleted.len());
        for p in &self.deleted {
            let _ = writeln!(out, "  - {}", p.display());
        }
        let _ = writeln!(out, "Skipped: {} entries", self.skipped.len());
        for (p, reason) in &self.skipped {
            let _ = writeln!(out, "  - {}  [{reason}]", p.display());
        }
        let _ = writeln!(out, "Self-delete: {}", self.self_delete);
        out
    }
}

impl Default for CleanupReport {
    fn default() -> Self {
        Self::new()
    }
}

/// Try to delete `path`. Retries on ERROR_SHARING_VIOLATION / ACCESS_DENIED
/// with exponential backoff up to `RETRY_BUDGET`. ENOENT counts as success.
pub fn retry_delete_file(path: &Path) -> std::io::Result<()> {
    let start = Instant::now();
    let mut step_idx = 0usize;
    loop {
        match std::fs::remove_file(path) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) if is_transient_lock(&e) => {
                if start.elapsed() >= RETRY_BUDGET {
                    return Err(e);
                }
                let ms = RETRY_STEPS_MS[step_idx.min(RETRY_STEPS_MS.len() - 1)];
                std::thread::sleep(Duration::from_millis(ms));
                step_idx += 1;
            }
            Err(e) => return Err(e),
        }
    }
}

/// Remove a directory and all its contents, applying `retry_delete_file`
/// semantics to every entry. Files that can't be deleted are recorded in
/// `report.skipped` and we keep going — partial cleanup is better than
/// infinite spin.
pub fn retry_delete_dir_all(path: &Path, report: &mut CleanupReport) {
    if !path.exists() {
        return;
    }
    // If it's a reparse point (junction/symlink), delete the link itself —
    // do NOT recurse into the target.
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() {
            match retry_delete_dir_only(path) {
                Ok(()) => report.deleted.push(path.to_path_buf()),
                Err(e) => report.skipped.push((path.to_path_buf(), format!("{e}"))),
            }
            return;
        }
    }

    let read = match std::fs::read_dir(path) {
        Ok(r) => r,
        Err(e) => {
            report
                .skipped
                .push((path.to_path_buf(), format!("read_dir: {e}")));
            return;
        }
    };

    for entry in read.flatten() {
        let p = entry.path();
        let is_dir = entry
            .file_type()
            .map(|t| t.is_dir() && !t.is_symlink())
            .unwrap_or(false);
        if is_dir {
            retry_delete_dir_all(&p, report);
        } else {
            match retry_delete_file(&p) {
                Ok(()) => report.deleted.push(p),
                Err(e) => report.skipped.push((p, format!("{e}"))),
            }
        }
    }

    match retry_delete_dir_only(path) {
        Ok(()) => report.deleted.push(path.to_path_buf()),
        Err(e) => report.skipped.push((path.to_path_buf(), format!("{e}"))),
    }
}

/// Like `retry_delete_file` but calls `remove_dir` (must be empty).
pub fn retry_delete_dir_only(path: &Path) -> std::io::Result<()> {
    let start = Instant::now();
    let mut step_idx = 0usize;
    loop {
        match std::fs::remove_dir(path) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) if is_transient_lock(&e) || is_not_empty(&e) => {
                if start.elapsed() >= RETRY_BUDGET {
                    return Err(e);
                }
                let ms = RETRY_STEPS_MS[step_idx.min(RETRY_STEPS_MS.len() - 1)];
                std::thread::sleep(Duration::from_millis(ms));
                step_idx += 1;
            }
            Err(e) => return Err(e),
        }
    }
}

fn is_transient_lock(e: &std::io::Error) -> bool {
    matches!(
        e.raw_os_error(),
        Some(ERROR_SHARING_VIOLATION) | Some(ERROR_ACCESS_DENIED) | Some(ERROR_LOCK_VIOLATION)
    )
}

fn is_not_empty(e: &std::io::Error) -> bool {
    matches!(e.raw_os_error(), Some(ERROR_DIR_NOT_EMPTY))
}

// ---------------------------------------------------------------------------
// Self-delete tiers (in order, fall through on failure):
//   1. POSIX unlink (Win10 1809+, NTFS). No admin, vanishes on process exit.
//   2. MoveFileEx delay-until-reboot. Needs admin (writes HKLM).
//   3. Give up; report LeftBehind.
// ---------------------------------------------------------------------------

#[cfg(windows)]
pub fn delete_self_exe() -> SelfDeleteOutcome {
    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => return SelfDeleteOutcome::LeftBehind(format!("current_exe: {e}")),
    };

    // Tier 1: POSIX unlink.
    if let Err(e) = posix_unlink_self(&exe) {
        eprintln!("posix self-delete failed: {e}; trying reboot-delete");
    } else {
        return SelfDeleteOutcome::PosixUnlinked;
    }

    // Tier 2: MoveFileEx delay-until-reboot. Needs admin. Avoid the old
    // cmd/bat retry helper because it can flash visible console windows
    // during UI-driven uninstall.
    match schedule_reboot_delete(&exe) {
        Ok(()) => SelfDeleteOutcome::ScheduledForReboot,
        Err(e) => SelfDeleteOutcome::LeftBehind(format!("reboot-delete failed: {e}")),
    }
}

#[cfg(windows)]
fn posix_unlink_self(exe: &Path) -> anyhow::Result<()> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{CloseHandle, GENERIC_READ};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, SetFileInformationByHandle, DELETE, FILE_ATTRIBUTE_NORMAL,
        FILE_DISPOSITION_INFO_EX, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows::Win32::Storage::FileSystem::{
        FileDispositionInfoEx, FILE_DISPOSITION_FLAG_DELETE, FILE_DISPOSITION_FLAG_POSIX_SEMANTICS,
    };

    let wide: Vec<u16> = exe
        .as_os_str()
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let handle = CreateFileW(
            PCWSTR(wide.as_ptr()),
            DELETE.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(FILE_ATTRIBUTE_NORMAL.0),
            None,
        )
        .map_err(|e| anyhow::anyhow!("CreateFileW: {e}"))?;
        let _ = GENERIC_READ; // silence unused

        let info = FILE_DISPOSITION_INFO_EX {
            Flags: windows::Win32::Storage::FileSystem::FILE_DISPOSITION_INFO_EX_FLAGS(
                FILE_DISPOSITION_FLAG_DELETE.0 | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS.0,
            ),
        };

        let res = SetFileInformationByHandle(
            handle,
            FileDispositionInfoEx,
            &info as *const _ as *const _,
            std::mem::size_of::<FILE_DISPOSITION_INFO_EX>() as u32,
        );

        let _ = CloseHandle(handle);
        res.map_err(|e| anyhow::anyhow!("SetFileInformationByHandle: {e}"))?;
    }
    Ok(())
}

#[cfg(windows)]
fn schedule_reboot_delete(exe: &Path) -> anyhow::Result<()> {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_DELAY_UNTIL_REBOOT};

    let wide: Vec<u16> = exe
        .as_os_str()
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        MoveFileExW(
            PCWSTR(wide.as_ptr()),
            PCWSTR::null(),
            MOVEFILE_DELAY_UNTIL_REBOOT,
        )
        .map_err(|e| anyhow::anyhow!("MoveFileExW: {e}"))?;
    }
    Ok(())
}

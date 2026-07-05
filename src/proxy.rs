//! Proxy-mode runtime: resolve the newest installed Codex.exe (self-healing
//! the `versions/current` junction if needed) and spawn it with the caller's
//! args + inherited env. We always spawn — Codex's own chromium-derived
//! ProcessSingleton handles the "already running, focus instead" case via
//! its named-pipe handoff. Before spawning we probe for the singleton's
//! hidden message window so we can warn the user if a *foreign* Codex
//! install is currently the lock holder (their window will get focused
//! instead of ours).

use crate::config::Config;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Same self-heal as the installer Launch button.
pub fn resolve_codex_exe(root: &Path, use_junction: bool) -> Option<PathBuf> {
    crate::codex_exe::latest_codex_exe(root, use_junction)
}

/// Identity of the process currently holding Codex's chromium ProcessSingleton.
#[derive(Debug, Clone)]
pub struct SingletonHolder {
    pub pid: u32,
    pub image_path: PathBuf,
}

/// Compute the userData path Codex's main process uses, mirroring the logic
/// in `bootstrap.js`:
///   1. `CODEX_ELECTRON_USER_DATA_PATH` env var (resolved absolute) if set,
///   2. otherwise `%APPDATA%\Codex` (production build flavor).
///
/// We don't reproduce the `agent` build-flavor branch — our launcher only
/// runs against the production desktop install.
pub fn codex_user_data_dir() -> Option<PathBuf> {
    if let Ok(v) = std::env::var("CODEX_ELECTRON_USER_DATA_PATH") {
        let v = v.trim();
        if !v.is_empty() {
            return Some(PathBuf::from(v));
        }
    }
    let appdata = std::env::var("APPDATA").ok()?;
    Some(PathBuf::from(appdata).join("Codex"))
}

/// Probe Codex's chromium ProcessSingleton. Looks for the hidden
/// message-only window with class `Chrome_MessageWindow` whose title equals
/// the userData path — that window is created by the lock-holding main
/// process on startup. Returns `None` if no responsive holder exists (in
/// which case spawning a fresh main is safe).
#[cfg(windows)]
pub fn find_singleton_holder(user_data_dir: &Path) -> Option<SingletonHolder> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowExW, GetWindowThreadProcessId, HWND_MESSAGE,
    };

    let class: Vec<u16> = "Chrome_MessageWindow"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let title: Vec<u16> = user_data_dir
        .as_os_str()
        .to_string_lossy()
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let hwnd = FindWindowExW(
            HWND_MESSAGE,
            windows::Win32::Foundation::HWND::default(),
            PCWSTR(class.as_ptr()),
            PCWSTR(title.as_ptr()),
        )
        .ok()?;
        if hwnd.0.is_null() {
            return None;
        }

        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return None;
        }

        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut size = buf.len() as u32;
        let r = QueryFullProcessImageNameW(
            h,
            PROCESS_NAME_FORMAT(0),
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut size,
        );
        let _ = CloseHandle(h);
        if r.is_err() {
            return None;
        }
        Some(SingletonHolder {
            pid,
            image_path: PathBuf::from(String::from_utf16_lossy(&buf[..size as usize])),
        })
    }
}

#[cfg(not(windows))]
pub fn find_singleton_holder(_user_data_dir: &Path) -> Option<SingletonHolder> {
    None
}

/// Spawn Codex.exe with forwarded args. Always spawns — Codex's own
/// ProcessSingleton handles the "already running" case via pipe handoff,
/// transferring focus to the lock-holder. Before spawning we check the
/// singleton; if a *foreign* install holds it, we MessageBox the user so
/// they understand why their click may surface that other install's window
/// instead of ours.
pub fn launch(root: &Path, cfg: &Config, forward_args: &[String]) -> Result<()> {
    let exe = resolve_codex_exe(root, cfg.use_current_junction)
        .ok_or_else(|| anyhow::anyhow!("no installed Codex.exe found under {}", root.display()))?;

    if let Some(udd) = codex_user_data_dir() {
        if let Some(holder) = find_singleton_holder(&udd) {
            let versions_root = root.join("versions");
            if !path_starts_with_ci(&holder.image_path, &versions_root) {
                let body = format!(
                    "Codex is currently running from a different install:\n\n\
                     {}\n\n\
                     OK — Launch this install anyway. Codex's single-instance handling \
                     may transfer focus to the running install instead of starting yours fresh.\n\n\
                     Kill other — Terminate the other Codex (and its child processes), \
                     then launch this install cleanly.",
                    holder.image_path.display()
                );
                let choice = crate::dialogs::two_button_choice(
                    "Codex launcher",
                    "Another Codex installation is running",
                    &body,
                    "OK",
                    "Kill other",
                );
                // Esc/X dismissal = safe default (no kill). Only kill on an
                // explicit Secondary click.
                if choice == crate::dialogs::DialogChoice::Secondary {
                    kill_foreign_codex(&holder, &versions_root);
                }
            }
        }
    }

    // Working dir = the versioned install dir so relative resource lookups
    // (Electron's default) resolve against the app root.
    let working_dir = exe.parent().unwrap_or(root);
    std::process::Command::new(&exe)
        .args(forward_args)
        .current_dir(working_dir)
        .spawn()
        .with_context(|| format!("spawning {}", exe.display()))?;
    Ok(())
}

/// Terminate every `Codex.exe` process whose image path is NOT under
/// `versions_root`. We confirm the holder is foreign first, then sweep
/// every other Codex.exe (children share the same foreign image). Waits
/// up to 5s per PID for exit so the new spawn doesn't race a still-alive
/// holder.
#[cfg(windows)]
fn kill_foreign_codex(holder: &SingletonHolder, versions_root: &Path) {
    let mut to_kill = Vec::new();
    for pid in find_codex_pids() {
        match process_image_path(pid) {
            Some(img) if !path_starts_with_ci(&img, versions_root) => to_kill.push(pid),
            None if pid == holder.pid => to_kill.push(pid), // confirmed foreign, can't query
            _ => {}
        }
    }
    terminate_pids(&to_kill, 5000);
}

/// Case-insensitive `Path::starts_with` for Windows paths. `QueryFullProcessImageNameW`
/// and `current_exe()` may return paths with differing casing (e.g. `C:\Program Files\...`
/// vs `C:\PROGRA~1\...` — though we don't handle short names here, only case).
/// Lowercasing is sufficient for the common collision: `C:\Users\X\...` vs
/// `C:\users\x\...`.
fn path_starts_with_ci(path: &Path, prefix: &Path) -> bool {
    let p = path.to_string_lossy().to_lowercase();
    let pre = prefix.to_string_lossy().to_lowercase();
    // Match on path components, not raw substring — avoid `C:\foo` matching
    // `C:\foobar`. Append a separator to the prefix and check prefix-of, or
    // accept exact equality.
    if p == pre {
        return true;
    }
    let pre_sep = if pre.ends_with('\\') || pre.ends_with('/') {
        pre
    } else {
        format!("{pre}\\")
    };
    p.starts_with(&pre_sep)
}

#[cfg(not(windows))]
fn kill_foreign_codex(_holder: &SingletonHolder, _versions_root: &Path) {}

/// PIDs of every Codex-named process whose image is under `versions_root`
/// (i.e. belongs to *this* install — main, renderers, GPU, utility, the
/// lowercase CLI helper at `resources/codex.exe`, etc.). Used by the
/// uninstaller to terminate only our processes, not foreign installs or
/// unrelated `codex.exe` binaries.
#[cfg(windows)]
pub fn find_our_codex_pids(versions_root: &Path) -> Vec<u32> {
    find_codex_pids()
        .into_iter()
        .filter(|&pid| {
            process_image_path(pid)
                .map(|img| path_starts_with_ci(&img, versions_root))
                .unwrap_or(false)
        })
        .collect()
}

#[cfg(not(windows))]
pub fn find_our_codex_pids(_versions_root: &Path) -> Vec<u32> {
    Vec::new()
}

/// Get the full image path for `pid`, or `None` if we can't query it.
#[cfg(windows)]
fn process_image_path(pid: u32) -> Option<PathBuf> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut size = buf.len() as u32;
        let r = QueryFullProcessImageNameW(
            h,
            PROCESS_NAME_FORMAT(0),
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut size,
        );
        let _ = CloseHandle(h);
        if r.is_err() {
            return None;
        }
        Some(PathBuf::from(String::from_utf16_lossy(
            &buf[..size as usize],
        )))
    }
}

/// Walk the process table collecting PIDs of every process named `Codex.exe`.
/// Electron apps fork multiple processes (main + renderer + GPU + utility),
/// all typically sharing the same exe name — callers that intend to terminate
/// Codex should kill every PID returned here, not just the first.
///
/// We skip our own PID so a hypothetical rename of the launcher to Codex.exe
/// wouldn't self-match.
#[cfg(windows)]
pub fn find_codex_pids() -> Vec<u32> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    let target = "codex.exe";
    let current_pid = std::process::id();
    let mut pids = Vec::new();

    unsafe {
        let snap = match CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            Ok(h) => h,
            Err(_) => return pids,
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snap, &mut entry).is_ok() {
            loop {
                if entry.th32ProcessID != current_pid {
                    let end = entry
                        .szExeFile
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(entry.szExeFile.len());
                    let name =
                        String::from_utf16_lossy(&entry.szExeFile[..end]).to_ascii_lowercase();
                    if name == target {
                        pids.push(entry.th32ProcessID);
                    }
                }
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    pids
}

#[cfg(not(windows))]
pub fn find_codex_pids() -> Vec<u32> {
    Vec::new()
}

/// Terminate every PID in `pids` and wait up to `wait_ms` total for each to
/// exit so file locks release before we try to delete the exes. Silently
/// skips PIDs that we can't open (already dead, access denied).
#[cfg(windows)]
pub fn terminate_pids(pids: &[u32], wait_ms: u32) {
    use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{
        OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
    };

    for &pid in pids {
        unsafe {
            let handle = match OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, false, pid) {
                Ok(h) => h,
                Err(_) => continue,
            };
            let _ = TerminateProcess(handle, 1);
            let wait_result = WaitForSingleObject(handle, wait_ms);
            if wait_result != WAIT_OBJECT_0 {
                eprintln!("warn: pid {pid} didn't exit within {wait_ms}ms");
            }
            let _ = CloseHandle(handle);
        }
    }
}

#[cfg(not(windows))]
pub fn terminate_pids(_pids: &[u32], _wait_ms: u32) {}

//! Launcher self-update worker.
//!
//! Flow on click "Update launcher":
//!   1. Download `codex-launcher.new.exe` next to the running exe.
//!   2. Verify against the published `.sha256`.
//!   3. Smoke-test: spawn `codex-launcher.new.exe --self-test` with a short
//!      timeout. Catches the catastrophic local-environment failures
//!      (corrupt download, AV quarantine, missing runtime DLL, architecture
//!      mismatch) before we touch the running launcher. The new launcher
//!      handles `--self-test` very early (before cleanup, mode detection,
//!      UI, network, update logic) and exits 0.
//!   4. Atomic-swap: rename running exe to `.old.exe`, move `.new.exe` into
//!      place. Windows allows renaming a running exe; deleting it would
//!      fail, hence the rename-then-replace dance.
//!   5. The running (old) launcher keeps working until the user picks
//!      "立即重启": `spawn_restart` starts the new exe with
//!      `--restart-after <pid>`, which waits for this process to exit before
//!      claiming the single-instance mutex. The next startup removes
//!      `.old.exe` once the previous process has released it.
//!
//! Files use a `.new.exe` / `.old.exe` suffix (rather than `.exe.new` /
//! `.exe.old`) so Windows recognizes them as executables — the smoke-test
//! step depends on being able to spawn the file.
//!
//! Elevation: System installs (Program Files) need admin to write the
//! launcher. Caller checks `elevate::is_elevated()` and re-spawns with
//! `--auto-self-update <ver>` if needed; this module assumes it has write
//! permission to the install root.

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;
use std::process::Command;

use crate::updater::LAUNCHER_REPO;

#[derive(Debug, Clone)]
pub enum LauncherUpdateMsg {
    Phase {
        phase: String,
        detail: String,
    },
    /// Fraction in [0.0, 1.0]. `None` means indeterminate.
    Progress(Option<f32>),
    /// Swap succeeded; `version` is the launcher now on disk. The running
    /// process is still the old build until it restarts.
    Done {
        version: String,
    },
    Error(String),
}

/// Run a full self-update for `target_version` (e.g. "0.2.0", no leading v).
/// Sends progress on `on_msg`. Always emits exactly one terminal `Done` or
/// `Error` as the final message.
pub fn apply(target_version: &str, on_msg: impl Fn(LauncherUpdateMsg) + Send + 'static) {
    match apply_inner(target_version, &on_msg) {
        Ok(()) => on_msg(LauncherUpdateMsg::Done {
            version: target_version.to_string(),
        }),
        Err(e) => on_msg(LauncherUpdateMsg::Error(format!("{e:#}"))),
    }
}

fn apply_inner(target_version: &str, on_msg: &dyn Fn(LauncherUpdateMsg)) -> Result<()> {
    let running = std::env::current_exe().context("current_exe()")?;
    let dir = running
        .parent()
        .ok_or_else(|| anyhow::anyhow!("running exe has no parent directory"))?
        .to_path_buf();

    let new_path = dir.join("codex-launcher.new.exe");
    let old_path = dir.join("codex-launcher.old.exe");

    // If a previous interrupted update left a `.new.exe` lying around, drop
    // it first — we're about to write a fresh one. A prior `.old.exe` may
    // still be locked by the previous launcher process, so the swap replaces
    // it when possible and normal startup retries cleanup later.
    let _ = std::fs::remove_file(&new_path);

    let tag = format!("v{target_version}");
    let exe_url =
        format!("https://github.com/{LAUNCHER_REPO}/releases/download/{tag}/codex-launcher.exe");
    let sha_url = format!(
        "https://github.com/{LAUNCHER_REPO}/releases/download/{tag}/codex-launcher.exe.sha256"
    );

    on_msg(LauncherUpdateMsg::Phase {
        phase: "Downloading launcher".into(),
        detail: format!("下载 {tag}"),
    });
    on_msg(LauncherUpdateMsg::Progress(Some(0.0)));

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .user_agent(concat!("codex-windows-updater/", env!("CARGO_PKG_VERSION")))
        .build()?;

    download_to_file(&client, &exe_url, &new_path, on_msg)
        .with_context(|| format!("downloading {exe_url}"))?;

    on_msg(LauncherUpdateMsg::Phase {
        phase: "Verifying".into(),
        detail: "核对发布页提供的 SHA-256 校验值".into(),
    });
    on_msg(LauncherUpdateMsg::Progress(None));

    let expected_hex =
        fetch_expected_sha(&client, &sha_url).with_context(|| format!("fetching {sha_url}"))?;
    let actual_hex = hash_file(&new_path).context("hashing downloaded file")?;
    if !actual_hex.eq_ignore_ascii_case(&expected_hex) {
        let _ = std::fs::remove_file(&new_path);
        anyhow::bail!(
            "SHA-256 mismatch: expected {expected_hex}, got {actual_hex}. \
             Aborting; running launcher untouched."
        );
    }

    on_msg(LauncherUpdateMsg::Phase {
        phase: "Smoke-testing".into(),
        detail: "确认新版本可以正常启动".into(),
    });
    on_msg(LauncherUpdateMsg::Progress(None));

    smoke_test(&new_path, std::time::Duration::from_secs(5)).inspect_err(|_| {
        // Failed smoke-test → drop the .new.exe and bail. Running launcher
        // is untouched.
        let _ = std::fs::remove_file(&new_path);
    })?;

    on_msg(LauncherUpdateMsg::Phase {
        phase: "Installing".into(),
        detail: "替换启动器文件".into(),
    });
    on_msg(LauncherUpdateMsg::Progress(None));

    swap_running(&running, &new_path, &old_path).context("replacing running launcher")?;

    Ok(())
}

/// Spawn `codex-launcher.new.exe --self-test` and wait up to `timeout`. The
/// new launcher must short-circuit `--self-test` before any side-effecting
/// code (cleanup, mode detection, UI, network) and exit 0. Any other
/// outcome — non-zero exit, timeout, or spawn failure — is a smoke-test
/// failure and we abort the swap.
fn smoke_test(new_exe: &Path, timeout: std::time::Duration) -> Result<()> {
    use std::process::{Command, Stdio};

    let mut child = Command::new(new_exe)
        .arg("--self-test")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("spawning {} --self-test", new_exe.display()))?;

    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait()? {
            Some(status) => {
                if status.success() {
                    return Ok(());
                }
                anyhow::bail!(
                    "smoke-test failed: new launcher exited with {status}. \
                     Aborting; running launcher untouched."
                );
            }
            None => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    anyhow::bail!(
                        "smoke-test timed out after {:?}. Aborting; running launcher untouched.",
                        timeout
                    );
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
    }
}

fn download_to_file(
    client: &reqwest::blocking::Client,
    url: &str,
    dest: &Path,
    on_msg: &dyn Fn(LauncherUpdateMsg),
) -> Result<()> {
    let mut resp = client.get(url).send()?.error_for_status()?;
    let total = resp.content_length();
    let mut out =
        std::fs::File::create(dest).with_context(|| format!("creating {}", dest.display()))?;
    let mut buf = [0u8; 64 * 1024];
    let mut done: u64 = 0;
    loop {
        let n = resp.read(&mut buf)?;
        if n == 0 {
            break;
        }
        std::io::Write::write_all(&mut out, &buf[..n])?;
        done += n as u64;
        let detail = match total {
            Some(t) => format!("{} / {} KB", done / 1024, t / 1024),
            None => format!("{} KB", done / 1024),
        };
        on_msg(LauncherUpdateMsg::Phase {
            phase: "Downloading launcher".into(),
            detail,
        });
        on_msg(LauncherUpdateMsg::Progress(
            total
                .filter(|t| *t > 0)
                .map(|t| (done as f32 / t as f32).clamp(0.0, 1.0)),
        ));
    }
    Ok(())
}

fn fetch_expected_sha(client: &reqwest::blocking::Client, url: &str) -> Result<String> {
    // File contents look like `<hex>  codex-launcher.exe`. Take the first
    // whitespace-delimited token.
    let body = client.get(url).send()?.error_for_status()?.text()?;
    let token = body
        .split_whitespace()
        .next()
        .ok_or_else(|| anyhow::anyhow!("empty .sha256 file"))?;
    if token.len() != 64 || !token.chars().all(|c| c.is_ascii_hexdigit()) {
        anyhow::bail!("malformed sha256 token: {token}");
    }
    Ok(token.to_string())
}

fn hash_file(path: &Path) -> Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex(&h.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Rename `running` → `running.old`, then move `new` → `running`. Tolerates
/// a stale `.old` from a prior update by replacing it. Windows permits
/// renaming an exe while it's executing, but not deletion — that's why we
/// rename instead.
fn swap_running(running: &Path, new_path: &Path, old_path: &Path) -> Result<()> {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_REPLACE_EXISTING};

    let running_w = wide(running);
    let new_w = wide(new_path);
    let old_w = wide(old_path);

    unsafe {
        MoveFileExW(
            PCWSTR(running_w.as_ptr()),
            PCWSTR(old_w.as_ptr()),
            MOVEFILE_REPLACE_EXISTING,
        )
        .with_context(|| format!("renaming {} → {}", running.display(), old_path.display()))?;

        if let Err(e) = MoveFileExW(
            PCWSTR(new_w.as_ptr()),
            PCWSTR(running_w.as_ptr()),
            MOVEFILE_REPLACE_EXISTING,
        ) {
            // Roll back: put the old launcher back so we don't leave the
            // user with no launcher at the canonical path.
            let _ = MoveFileExW(
                PCWSTR(old_w.as_ptr()),
                PCWSTR(running_w.as_ptr()),
                MOVEFILE_REPLACE_EXISTING,
            );
            return Err(e)
                .with_context(|| format!("moving {} → {}", new_path.display(), running.display()));
        }
    }
    Ok(())
}

fn wide(p: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    p.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// Flag passed to the freshly installed launcher so it waits for the old
/// process to exit before claiming the single-instance mutex.
pub const RESTART_AFTER_FLAG: &str = "--restart-after";

/// Start `exe` (the path the old launcher was running from, now holding the
/// new build) and ask it to wait for the current process to exit.
pub fn spawn_restart(exe: &Path) -> Result<()> {
    Command::new(exe)
        .arg(RESTART_AFTER_FLAG)
        .arg(std::process::id().to_string())
        .spawn()
        .with_context(|| format!("starting {}", exe.display()))?;
    Ok(())
}

/// Read the pid from `--restart-after <pid>` anywhere in `args`.
pub fn restart_after_pid(args: impl IntoIterator<Item = String>) -> Option<u32> {
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if arg == RESTART_AFTER_FLAG {
            return args.next().and_then(|pid| pid.parse().ok());
        }
    }
    None
}

/// Block until process `pid` exits or `timeout` elapses. Returns true when the
/// process is gone (including when it had already exited or cannot be opened).
pub fn wait_for_process_exit(pid: u32, timeout: std::time::Duration) -> bool {
    use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{
        OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
    };

    let Ok(handle) = (unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }) else {
        return true;
    };
    let millis = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX);
    let result = unsafe { WaitForSingleObject(handle, millis) };
    unsafe {
        let _ = CloseHandle(handle);
    }
    result == WAIT_OBJECT_0
}

/// Best-effort cleanup for launcher artifacts left by a previous update.
pub fn cleanup_stale_launchers(dir: &Path) {
    for name in ["codex-launcher.new.exe", "codex-launcher.old.exe"] {
        let path = dir.join(name);
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                // The previous launcher or security software may still hold
                // the file. Normal startup retries this cleanup next time.
                eprintln!("启动器残留清理失败（{}）：{error}", path.display());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{cleanup_stale_launchers, restart_after_pid, wait_for_process_exit};
    use std::time::Duration;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn restart_flag_yields_the_previous_launcher_pid() {
        assert_eq!(
            restart_after_pid(args(&["--restart-after", "4242"])),
            Some(4242)
        );
        assert_eq!(restart_after_pid(args(&["--restart-after"])), None);
        assert_eq!(restart_after_pid(args(&["--restart-after", "abc"])), None);
        assert_eq!(restart_after_pid(args(&["--launch-latest"])), None);
    }

    #[test]
    fn waits_until_the_previous_launcher_exits() {
        let child = std::process::Command::new("cmd")
            .args(["/C", "ping -n 2 127.0.0.1 >nul"])
            .spawn()
            .expect("spawn helper process");
        let pid = child.id();

        assert!(wait_for_process_exit(pid, Duration::from_secs(10)));
    }

    #[test]
    fn stops_waiting_after_the_timeout() {
        let mut child = std::process::Command::new("cmd")
            .args(["/C", "ping -n 6 127.0.0.1 >nul"])
            .spawn()
            .expect("spawn helper process");

        assert!(!wait_for_process_exit(
            child.id(),
            Duration::from_millis(100)
        ));
        let _ = child.kill();
        let _ = child.wait();
    }

    #[test]
    fn startup_cleanup_removes_new_and_old_launcher_artifacts() {
        let root =
            std::env::temp_dir().join(format!("codex-launcher-cleanup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        let new_path = root.join("codex-launcher.new.exe");
        let old_path = root.join("codex-launcher.old.exe");
        std::fs::write(&new_path, b"new").unwrap();
        std::fs::write(&old_path, b"old").unwrap();

        cleanup_stale_launchers(&root);
        cleanup_stale_launchers(&root);

        assert!(!new_path.exists());
        assert!(!old_path.exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn startup_cleanup_retries_after_a_file_becomes_deletable() {
        let root = std::env::temp_dir().join(format!(
            "codex-launcher-cleanup-retry-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        let old_path = root.join("codex-launcher.old.exe");
        std::fs::create_dir_all(&old_path).unwrap();

        cleanup_stale_launchers(&root);
        assert!(old_path.exists());

        std::fs::remove_dir(&old_path).unwrap();
        std::fs::write(&old_path, b"old").unwrap();
        cleanup_stale_launchers(&root);

        assert!(!old_path.exists());
        let _ = std::fs::remove_dir_all(root);
    }
}

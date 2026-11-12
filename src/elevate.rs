//! UAC elevation helpers. `is_elevated()` inspects the current token;
//! `respawn_elevated()` relaunches the same exe with `runas` verb and
//! given CLI args — caller should exit immediately after.

use anyhow::{Context, Result};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetExitCodeProcess, OpenProcessToken, WaitForSingleObject,
};
use windows::Win32::UI::Shell::{
    ShellExecuteExW, ShellExecuteW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_NORMAL;

pub fn is_elevated() -> bool {
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elev = TOKEN_ELEVATION::default();
        let mut ret_len = 0u32;
        let result = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elev as *mut _ as *mut _),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut ret_len,
        );
        let _ = CloseHandle(token);
        result.is_ok() && elev.TokenIsElevated != 0
    }
}

/// Relaunch the current exe elevated, passing `args` as the command line.
/// Returns Ok if the shell launched the new process (UAC accepted); caller
/// should then exit the current process so only the elevated one remains.
pub fn respawn_elevated(args: &str) -> Result<()> {
    let exe = std::env::current_exe().context("current_exe")?;
    let exe_w = wide(&exe.to_string_lossy());
    let verb = wide("runas");
    let args_w = wide(args);
    unsafe {
        let h = ShellExecuteW(
            HWND::default(),
            PCWSTR(verb.as_ptr()),
            PCWSTR(exe_w.as_ptr()),
            PCWSTR(args_w.as_ptr()),
            PCWSTR::null(),
            SW_NORMAL,
        );
        // ShellExecuteW returns HINSTANCE; a value <= 32 means failure
        // (including ERROR_CANCELLED when the user declines UAC).
        let code = h.0 as isize;
        if code <= 32 {
            anyhow::bail!("ShellExecuteW runas failed (code {code})");
        }
    }
    Ok(())
}

/// Run the current executable elevated and wait for the one-shot helper to
/// finish. The caller stays unelevated and can refresh its state afterward.
pub fn respawn_elevated_wait(args: &str) -> Result<u32> {
    respawn_elevated_wait_with_progress(args, || {})
}

pub fn respawn_elevated_wait_with_progress(args: &str, mut on_poll: impl FnMut()) -> Result<u32> {
    let exe = std::env::current_exe().context("current_exe")?;
    let exe_w = wide(&exe.to_string_lossy());
    let verb = wide("runas");
    let args_w = wide(args);
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        hwnd: HWND::default(),
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(exe_w.as_ptr()),
        lpParameters: PCWSTR(args_w.as_ptr()),
        nShow: SW_NORMAL.0,
        ..Default::default()
    };

    unsafe {
        ShellExecuteExW(&mut info).context("ShellExecuteExW runas")?;
        if info.hProcess.is_invalid() {
            anyhow::bail!("elevated helper did not return a process handle");
        }

        let result = wait_for_process(info.hProcess, &mut on_poll);
        let _ = CloseHandle(info.hProcess);
        result
    }
}

fn wait_for_process(process: HANDLE, on_poll: &mut dyn FnMut()) -> Result<u32> {
    loop {
        let wait = unsafe { WaitForSingleObject(process, 200) };
        if wait != WAIT_OBJECT_0 && wait != WAIT_TIMEOUT {
            anyhow::bail!("waiting for elevated helper failed");
        }
        // Drain the last progress record before the caller publishes Done/Error.
        on_poll();
        if wait == WAIT_OBJECT_0 {
            let mut exit_code = 1;
            unsafe { GetExitCodeProcess(process, &mut exit_code) }.context("GetExitCodeProcess")?;
            return Ok(exit_code);
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elevated_progress::{ProgressReader, ProgressWriter};
    use crate::installer::InstallMsg;
    use std::os::windows::{io::AsRawHandle, process::CommandExt};
    use std::path::PathBuf;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    #[test]
    fn progress_is_polled_before_the_helper_exits_and_drained_at_exit() {
        const CHILD_PATH: &str = "CODEX_TEST_PROGRESS_RESULT_PATH";
        if let Some(path) = std::env::var_os(CHILD_PATH) {
            let result = PathBuf::from(path);
            let mut writer = ProgressWriter::open(&result).unwrap();
            for (detail, value) in [("first", 0.6), ("retry", 0.0), ("last", 1.0)] {
                writer
                    .publish(InstallMsg::ProgressDetail {
                        phase: "Downloading".into(),
                        detail: detail.into(),
                        progress: Some(value),
                    })
                    .unwrap();
                if detail != "last" {
                    let deadline = Instant::now() + Duration::from_secs(10);
                    while std::fs::read_to_string(result.with_extension("ack")).unwrap_or_default()
                        != detail
                    {
                        assert!(Instant::now() < deadline, "parent did not observe {detail}");
                        std::thread::sleep(Duration::from_millis(10));
                    }
                }
            }
            return;
        }
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let result = std::env::temp_dir().join(format!(
            "codex-progress-poll-{}-{nonce}.json",
            std::process::id()
        ));
        let mut reader = ProgressReader::create(&result).unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "elevate::tests::progress_is_polled_before_the_helper_exits_and_drained_at_exit",
                "--nocapture",
            ])
            .env(CHILD_PATH, &result)
            .creation_flags(0x08000000)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let handle = HANDLE(child.as_raw_handle());
        let mut seen = Vec::new();
        let exit = wait_for_process(handle, &mut || {
            if let Some(InstallMsg::ProgressDetail {
                detail, progress, ..
            }) = reader.read_latest().unwrap()
            {
                if detail != "last" {
                    assert!(
                        child.try_wait().unwrap().is_none(),
                        "progress must arrive while the helper is running"
                    );
                    std::fs::write(result.with_extension("ack"), &detail).unwrap();
                }
                seen.push((detail, progress));
            }
        });
        let output = child.wait_with_output().unwrap();
        let _ = std::fs::remove_file(result.with_extension("ack"));
        assert_eq!(
            exit.unwrap(),
            0,
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            seen,
            vec![
                ("first".into(), Some(0.6)),
                ("retry".into(), Some(0.0)),
                ("last".into(), Some(1.0))
            ]
        );
    }
}

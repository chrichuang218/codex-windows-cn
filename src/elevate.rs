//! UAC elevation helpers. `is_elevated()` inspects the current token;
//! `respawn_elevated()` relaunches the same exe with `runas` verb and
//! given CLI args — caller should exit immediately after.

use anyhow::{Context, Result};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::UI::Shell::ShellExecuteW;
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

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

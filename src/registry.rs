//! Add/Remove Programs registration — writes our `Uninstall\<key>` so
//! Windows Settings → Apps lists us and runs our `--uninstall`. Key name
//! is unique to this project to avoid colliding with anything OpenAI ships.
//!
//! User-mode installs write to HKCU (no admin needed). System-mode writes
//! to HKLM and therefore requires elevation — caller must already be
//! running elevated.

use crate::config::InstallMode;
use anyhow::{Context, Result};
use std::path::Path;
use windows::core::PCWSTR;
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
    HKEY_LOCAL_MACHINE, KEY_WRITE, REG_CREATE_KEY_DISPOSITION, REG_DWORD, REG_OPTION_NON_VOLATILE,
    REG_SZ,
};

pub const UNINSTALL_SUBKEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Uninstall\CodexUnofficialUpdater";

fn root(mode: InstallMode) -> HKEY {
    match mode {
        InstallMode::System => HKEY_LOCAL_MACHINE,
        _ => HKEY_CURRENT_USER,
    }
}

pub struct UninstallEntry<'a> {
    pub display_name: &'a str,
    pub display_version: &'a str,
    pub publisher: &'a str,
    pub install_location: &'a Path,
    pub uninstall_string: String,
    pub display_icon: &'a Path,
}

pub fn write(mode: InstallMode, entry: &UninstallEntry) -> Result<()> {
    unsafe {
        let subkey = wide(UNINSTALL_SUBKEY);
        let mut hkey = HKEY::default();
        let mut disp = REG_CREATE_KEY_DISPOSITION::default();
        let rc = RegCreateKeyExW(
            root(mode),
            PCWSTR(subkey.as_ptr()),
            0,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut hkey,
            Some(&mut disp),
        );
        rc.ok().context("RegCreateKeyExW")?;

        let r = (|| -> Result<()> {
            set_sz(hkey, "DisplayName", entry.display_name)?;
            set_sz(hkey, "DisplayVersion", entry.display_version)?;
            set_sz(hkey, "Publisher", entry.publisher)?;
            set_sz(
                hkey,
                "InstallLocation",
                &entry.install_location.to_string_lossy(),
            )?;
            set_sz(hkey, "UninstallString", &entry.uninstall_string)?;
            set_sz(hkey, "DisplayIcon", &entry.display_icon.to_string_lossy())?;
            set_dword(hkey, "NoModify", 1)?;
            set_dword(hkey, "NoRepair", 1)?;
            Ok(())
        })();
        let _ = RegCloseKey(hkey);
        r
    }
}

pub fn remove(mode: InstallMode) -> Result<()> {
    unsafe {
        let subkey = wide(UNINSTALL_SUBKEY);
        let _ = RegDeleteTreeW(root(mode), PCWSTR(subkey.as_ptr()));
    }
    Ok(())
}

unsafe fn set_sz(hkey: HKEY, name: &str, value: &str) -> Result<()> {
    let name_w = wide(name);
    let value_w: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = std::slice::from_raw_parts(value_w.as_ptr() as *const u8, value_w.len() * 2);
    RegSetValueExW(hkey, PCWSTR(name_w.as_ptr()), 0, REG_SZ, Some(bytes))
        .ok()
        .with_context(|| format!("RegSetValueExW({name})"))
}

unsafe fn set_dword(hkey: HKEY, name: &str, value: u32) -> Result<()> {
    let name_w = wide(name);
    let bytes = value.to_le_bytes();
    RegSetValueExW(
        hkey,
        PCWSTR(name_w.as_ptr()),
        0,
        REG_DWORD,
        Some(&bytes[..]),
    )
    .ok()
    .with_context(|| format!("RegSetValueExW({name})"))
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

//! Start Menu `.lnk` creation + refresh via `IShellLinkW` / `IPersistFile`.
//!
//! Target = `<root>\codex-launcher.exe` (stable path — shortcut survives
//! version bumps). IconLocation points at a real `Codex.exe` so the Start
//! Menu renders Codex's own icon. On update we rewrite the shortcut to
//! retarget the icon at the newest version's `Codex.exe`.

use crate::config::InstallMode;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use windows::core::{Interface, GUID, PCWSTR};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, IPersistFile,
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
};
use windows::Win32::UI::Shell::{
    FOLDERID_CommonStartMenu, FOLDERID_StartMenu, IShellLinkW, SHGetKnownFolderPath, ShellLink,
    KF_FLAG_DEFAULT,
};

pub const SHORTCUT_NAME: &str = "Codex.lnk";

/// Where `<StartMenu>\Programs\Codex.lnk` resolves for a given install
/// mode. Portable mode returns `Ok(None)` — no Start Menu shortcut.
pub fn link_path(mode: InstallMode) -> Result<Option<PathBuf>> {
    let id = match mode {
        InstallMode::Portable => return Ok(None),
        InstallMode::User => FOLDERID_StartMenu,
        InstallMode::System => FOLDERID_CommonStartMenu,
    };
    Ok(Some(
        known_folder(&id)?.join("Programs").join(SHORTCUT_NAME),
    ))
}

fn known_folder(id: &GUID) -> Result<PathBuf> {
    unsafe {
        let pw = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).context("SHGetKnownFolderPath")?;
        let mut len = 0usize;
        while *pw.0.add(len) != 0 {
            len += 1;
        }
        let slice = std::slice::from_raw_parts(pw.0, len);
        let path = String::from_utf16_lossy(slice);
        CoTaskMemFree(Some(pw.0 as *mut _));
        Ok(PathBuf::from(path))
    }
}

/// Create or overwrite the `.lnk` at `link`. Caller supplies target exe +
/// icon source. Returns `Err` on COM failures — caller typically logs and
/// continues (shortcut is non-essential).
pub fn create_or_update(
    link: &Path,
    target_exe: &Path,
    icon_source: &Path,
    description: &str,
    working_dir: &Path,
) -> Result<()> {
    if let Some(parent) = link.parent() {
        std::fs::create_dir_all(parent)?;
    }
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let link_com: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
            .context("CoCreateInstance(ShellLink)")?;

        let target_w = wide(&target_exe.to_string_lossy());
        link_com.SetPath(PCWSTR(target_w.as_ptr()))?;

        let desc_w = wide(description);
        link_com.SetDescription(PCWSTR(desc_w.as_ptr()))?;

        let icon_w = wide(&icon_source.to_string_lossy());
        link_com.SetIconLocation(PCWSTR(icon_w.as_ptr()), 0)?;

        let wd_w = wide(&working_dir.to_string_lossy());
        link_com.SetWorkingDirectory(PCWSTR(wd_w.as_ptr()))?;

        let persist: IPersistFile = link_com.cast().context("QI IPersistFile")?;
        let path_w = wide(&link.to_string_lossy());
        persist
            .Save(PCWSTR(path_w.as_ptr()), true)
            .context("IPersistFile::Save")?;

        CoUninitialize();
    }
    Ok(())
}

pub fn remove(link: &Path) -> Result<()> {
    if link.exists() {
        std::fs::remove_file(link)?;
    }
    Ok(())
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

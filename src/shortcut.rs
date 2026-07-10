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
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, STGM_READ,
};
use windows::Win32::UI::Shell::{
    FOLDERID_CommonStartMenu, FOLDERID_Desktop, FOLDERID_PublicDesktop, FOLDERID_StartMenu,
    IShellLinkW, SHGetKnownFolderPath, ShellLink, KF_FLAG_DEFAULT, SLGP_RAWPATH,
};

pub const SHORTCUT_NAME: &str = "Codex Windows 中文助手.lnk";
pub const LEGACY_SHORTCUT_NAME: &str = "Codex.lnk";
pub const DESKTOP_SHORTCUT_NAME: &str = "ChatGPT.lnk";
pub const LEGACY_DESKTOP_SHORTCUT_NAME: &str = "ChatGPT（中文助手）.lnk";
pub const ASSISTANT_DESKTOP_SHORTCUT_NAME: &str = "Codex Windows 中文助手.lnk";

/// Where `<StartMenu>\Programs\Codex.lnk` resolves for a given install
/// mode. Portable mode returns `Ok(None)` — no Start Menu shortcut.
pub fn link_path(mode: InstallMode) -> Result<Option<PathBuf>> {
    Ok(start_menu_directory(mode)?.map(|directory| directory.join(SHORTCUT_NAME)))
}

pub fn legacy_link_path(mode: InstallMode) -> Result<Option<PathBuf>> {
    Ok(start_menu_directory(mode)?.map(|directory| directory.join(LEGACY_SHORTCUT_NAME)))
}

fn start_menu_directory(mode: InstallMode) -> Result<Option<PathBuf>> {
    let id = match mode {
        InstallMode::Portable => return Ok(None),
        InstallMode::User => FOLDERID_StartMenu,
        InstallMode::System => FOLDERID_CommonStartMenu,
    };
    Ok(Some(known_folder(&id)?.join("Programs")))
}

/// Where the direct-launch ChatGPT desktop shortcut lives. Portable and
/// current-user installs use the current user's possibly redirected Desktop;
/// system installs use the shared Public Desktop.
pub fn desktop_link_path(mode: InstallMode) -> Result<PathBuf> {
    Ok(desktop_directory(mode)?.join(DESKTOP_SHORTCUT_NAME))
}

pub fn legacy_desktop_link_path(mode: InstallMode) -> Result<PathBuf> {
    Ok(desktop_directory(mode)?.join(LEGACY_DESKTOP_SHORTCUT_NAME))
}

pub fn assistant_desktop_link_path(mode: InstallMode) -> Result<PathBuf> {
    Ok(desktop_directory(mode)?.join(ASSISTANT_DESKTOP_SHORTCUT_NAME))
}

fn desktop_directory(mode: InstallMode) -> Result<PathBuf> {
    let id = match mode {
        InstallMode::Portable | InstallMode::User => FOLDERID_Desktop,
        InstallMode::System => FOLDERID_PublicDesktop,
    };
    known_folder(&id)
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
    arguments: &str,
) -> Result<()> {
    if let Some(parent) = link.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let _com = ComInitialization::new();
    unsafe {
        let link_com: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
            .context("CoCreateInstance(ShellLink)")?;

        let target_w = wide(&target_exe.to_string_lossy());
        link_com.SetPath(PCWSTR(target_w.as_ptr()))?;

        let desc_w = wide(description);
        link_com.SetDescription(PCWSTR(desc_w.as_ptr()))?;

        let arguments_w = wide(arguments);
        link_com.SetArguments(PCWSTR(arguments_w.as_ptr()))?;

        let icon_w = wide(&icon_source.to_string_lossy());
        link_com.SetIconLocation(PCWSTR(icon_w.as_ptr()), 0)?;

        let wd_w = wide(&working_dir.to_string_lossy());
        link_com.SetWorkingDirectory(PCWSTR(wd_w.as_ptr()))?;

        let persist: IPersistFile = link_com.cast().context("QI IPersistFile")?;
        let path_w = wide(&link.to_string_lossy());
        persist
            .Save(PCWSTR(path_w.as_ptr()), true)
            .context("IPersistFile::Save")?;
    }
    Ok(())
}

pub fn is_owned(link: &Path, target_exe: &Path, arguments: &str) -> Result<bool> {
    if !link.is_file() {
        return Ok(false);
    }
    let (target, actual_arguments) = read_target_and_arguments(link)?;
    Ok(paths_equal(&target, target_exe) && actual_arguments.trim() == arguments)
}

fn read_target_and_arguments(link: &Path) -> Result<(PathBuf, String)> {
    let _com = ComInitialization::new();
    unsafe {
        let link_com: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
            .context("CoCreateInstance(ShellLink)")?;
        let persist: IPersistFile = link_com.cast().context("QI IPersistFile")?;
        let path_w = wide(&link.to_string_lossy());
        persist
            .Load(PCWSTR(path_w.as_ptr()), STGM_READ)
            .context("IPersistFile::Load")?;

        let mut target = vec![0u16; 32_768];
        link_com
            .GetPath(&mut target, std::ptr::null_mut(), SLGP_RAWPATH.0 as u32)
            .context("IShellLinkW::GetPath")?;
        let mut arguments = vec![0u16; 2_048];
        link_com
            .GetArguments(&mut arguments)
            .context("IShellLinkW::GetArguments")?;
        Ok((
            PathBuf::from(from_wide_buffer(&target)),
            from_wide_buffer(&arguments),
        ))
    }
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

fn from_wide_buffer(buffer: &[u16]) -> String {
    let len = buffer
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..len])
}

fn paths_equal(left: &Path, right: &Path) -> bool {
    if let (Ok(left), Ok(right)) = (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
        return left == right;
    }
    let normalize = |path: &Path| {
        path.to_string_lossy()
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_ascii_lowercase()
    };
    normalize(left) == normalize(right)
}

struct ComInitialization(bool);

impl ComInitialization {
    fn new() -> Self {
        Self(unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok() })
    }
}

impl Drop for ComInitialization {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        paths_equal, ASSISTANT_DESKTOP_SHORTCUT_NAME, DESKTOP_SHORTCUT_NAME,
        LEGACY_DESKTOP_SHORTCUT_NAME, LEGACY_SHORTCUT_NAME, SHORTCUT_NAME,
    };
    use std::path::Path;

    #[test]
    fn desktop_shortcut_uses_plain_chatgpt_name() {
        assert_eq!(SHORTCUT_NAME, "Codex Windows 中文助手.lnk");
        assert_eq!(LEGACY_SHORTCUT_NAME, "Codex.lnk");
        assert_eq!(DESKTOP_SHORTCUT_NAME, "ChatGPT.lnk");
        assert_eq!(LEGACY_DESKTOP_SHORTCUT_NAME, "ChatGPT（中文助手）.lnk");
        assert_eq!(
            ASSISTANT_DESKTOP_SHORTCUT_NAME,
            "Codex Windows 中文助手.lnk"
        );
    }

    #[test]
    fn ownership_path_comparison_is_case_insensitive() {
        assert!(paths_equal(
            Path::new(r"C:\Program Files\Codex\codex-launcher.exe"),
            Path::new(r"c:/program files/codex/CODEX-LAUNCHER.EXE")
        ));
    }
}

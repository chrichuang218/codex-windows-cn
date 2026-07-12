//! Add/Remove Programs registration — writes our `Uninstall\<key>` so
//! Windows Settings → Apps lists us and runs our `--uninstall`. Key name
//! is unique to this project to avoid colliding with anything OpenAI ships.
//!
//! User-mode installs write to HKCU (no admin needed). System-mode writes
//! to HKLM and therefore requires elevation — caller must already be
//! running elevated.

use crate::config::InstallMode;
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use windows::core::{HRESULT, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_NO_ASSOCIATION, ERROR_PATH_NOT_FOUND,
};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegGetValueW, RegOpenKeyExW, RegSetValueExW,
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WRITE, REG_CREATE_KEY_DISPOSITION,
    REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ,
};
use windows::Win32::UI::Shell::{
    AssocQueryStringW, SHChangeNotify, ASSOCF_NONE, ASSOCSTR_EXECUTABLE, SHCNE_ASSOCCHANGED,
    SHCNF_IDLIST,
};

pub const UNINSTALL_SUBKEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Uninstall\CodexUnofficialUpdater";
const PROTOCOL_OWNER_ID: &str = "chrichuang218/codex-windows-cn";
const PROTOCOL_OWNER_VALUE: &str = "CodexWindowsCnOwner";
const PROTOCOL_INSTALL_ROOT_VALUE: &str = "CodexWindowsCnInstallRoot";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolRegistration {
    Created,
    Refreshed,
    Unchanged,
    PreservedForeign,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolRemoval {
    Removed,
    NotFound,
    PreservedForeign,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexProtocolStatus {
    Missing,
    Ready,
    NeedsRepair,
    OtherOwner,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProtocolValues {
    command: String,
    icon: String,
}

impl ProtocolValues {
    fn new(executable: &Path) -> Self {
        let executable = executable.to_string_lossy();
        Self {
            command: format!(r#""{executable}" "%1""#),
            icon: format!(r#""{executable}",0"#),
        }
    }
}

#[derive(Debug, Default)]
struct ExistingProtocol {
    key_exists: bool,
    owner: Option<String>,
    install_root: Option<String>,
    command: Option<String>,
}

struct ProtocolRegistryLocation {
    hive: HKEY,
    scheme: String,
    subkey: String,
}

impl ProtocolRegistryLocation {
    fn new(hive: HKEY, scheme: &str) -> Self {
        Self {
            hive,
            scheme: scheme.to_string(),
            subkey: format!(r"Software\Classes\{scheme}"),
        }
    }

    fn icon_subkey(&self) -> String {
        format!(r"{}\DefaultIcon", self.subkey)
    }

    fn command_subkey(&self) -> String {
        format!(r"{}\shell\open\command", self.subkey)
    }
}

struct RegistryKey(HKEY);

impl Drop for RegistryKey {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

pub fn codex_protocol_status(
    mode: InstallMode,
    install_root: &Path,
    handler_exe: &Path,
) -> Result<CodexProtocolStatus> {
    codex_protocol_status_at(
        &ProtocolRegistryLocation::new(root(mode), "codex"),
        install_root,
        handler_exe,
    )
}

pub fn register_codex_protocol(
    mode: InstallMode,
    install_root: &Path,
    handler_exe: &Path,
) -> Result<ProtocolRegistration> {
    register_codex_protocol_at(
        &ProtocolRegistryLocation::new(root(mode), "codex"),
        install_root,
        handler_exe,
        false,
    )
}

pub fn replace_codex_protocol(
    mode: InstallMode,
    install_root: &Path,
    handler_exe: &Path,
) -> Result<ProtocolRegistration> {
    register_codex_protocol_at(
        &ProtocolRegistryLocation::new(root(mode), "codex"),
        install_root,
        handler_exe,
        true,
    )
}

pub fn remove_codex_protocol_if_owned(
    mode: InstallMode,
    install_root: &Path,
) -> Result<ProtocolRemoval> {
    remove_codex_protocol_at(
        &ProtocolRegistryLocation::new(root(mode), "codex"),
        install_root,
    )
}

fn codex_protocol_status_at(
    location: &ProtocolRegistryLocation,
    install_root: &Path,
    handler_exe: &Path,
) -> Result<CodexProtocolStatus> {
    validate_protocol_handler(install_root, handler_exe)?;
    let existing = read_existing_protocol(location)?;
    Ok(classify_protocol(
        &existing,
        install_root,
        &ProtocolValues::new(handler_exe),
    ))
}

fn register_codex_protocol_at(
    location: &ProtocolRegistryLocation,
    install_root: &Path,
    handler_exe: &Path,
    replace_foreign: bool,
) -> Result<ProtocolRegistration> {
    validate_protocol_handler(install_root, handler_exe)?;
    let desired = ProtocolValues::new(handler_exe);
    let existing = read_existing_protocol(location)?;
    let status = classify_protocol(&existing, install_root, &desired);
    if status == CodexProtocolStatus::Ready {
        return Ok(ProtocolRegistration::Unchanged);
    }
    if status == CodexProtocolStatus::OtherOwner && !replace_foreign {
        return Ok(ProtocolRegistration::PreservedForeign);
    }
    if status == CodexProtocolStatus::Missing {
        if let Some(effective) = effective_protocol_handler(&location.scheme)? {
            if !protocol_handler_belongs_to(install_root, &effective) && !replace_foreign {
                return Ok(ProtocolRegistration::PreservedForeign);
            }
        }
    }
    if status == CodexProtocolStatus::OtherOwner && replace_foreign {
        delete_tree(location.hive, &location.subkey)?;
    }

    write_protocol(location, install_root, &desired)?;
    notify_association_changed();
    Ok(if status == CodexProtocolStatus::Missing {
        ProtocolRegistration::Created
    } else {
        ProtocolRegistration::Refreshed
    })
}

fn remove_codex_protocol_at(
    location: &ProtocolRegistryLocation,
    install_root: &Path,
) -> Result<ProtocolRemoval> {
    let existing = read_existing_protocol(location)?;
    if !existing.key_exists {
        return Ok(ProtocolRemoval::NotFound);
    }
    if !protocol_is_owned(&existing, install_root) {
        return Ok(ProtocolRemoval::PreservedForeign);
    }
    delete_tree(location.hive, &location.subkey)?;
    notify_association_changed();
    Ok(ProtocolRemoval::Removed)
}

fn validate_protocol_handler(install_root: &Path, handler_exe: &Path) -> Result<()> {
    if !handler_exe.is_absolute() {
        bail!(
            "protocol handler must be an absolute path: {}",
            handler_exe.display()
        );
    }
    if !handler_exe.is_file() {
        bail!("protocol handler does not exist: {}", handler_exe.display());
    }
    if !protocol_handler_belongs_to(install_root, handler_exe) {
        bail!(
            "protocol handler is outside the managed versions directory: {}",
            handler_exe.display()
        );
    }
    Ok(())
}

fn read_existing_protocol(location: &ProtocolRegistryLocation) -> Result<ExistingProtocol> {
    if !key_exists(location.hive, &location.subkey)? {
        return Ok(ExistingProtocol::default());
    }
    Ok(ExistingProtocol {
        key_exists: true,
        owner: read_sz_at(location.hive, &location.subkey, Some(PROTOCOL_OWNER_VALUE))?,
        install_root: read_sz_at(
            location.hive,
            &location.subkey,
            Some(PROTOCOL_INSTALL_ROOT_VALUE),
        )?,
        command: read_sz_at(location.hive, &location.command_subkey(), None)?,
    })
}

fn protocol_is_owned(existing: &ExistingProtocol, install_root: &Path) -> bool {
    if existing
        .owner
        .as_deref()
        .is_some_and(|owner| owner != PROTOCOL_OWNER_ID)
        || existing
            .install_root
            .as_deref()
            .is_some_and(|root| !paths_equal(Path::new(root), install_root))
    {
        return false;
    }
    let marker_owned = existing.owner.as_deref() == Some(PROTOCOL_OWNER_ID)
        && existing
            .install_root
            .as_deref()
            .is_some_and(|root| paths_equal(Path::new(root), install_root));
    match existing.command.as_deref() {
        Some(command) => protocol_command_executable(command)
            .is_some_and(|executable| protocol_handler_belongs_to(install_root, &executable)),
        None => marker_owned,
    }
}

fn write_protocol(
    location: &ProtocolRegistryLocation,
    install_root: &Path,
    values: &ProtocolValues,
) -> Result<()> {
    let root_key = create_key(location.hive, &location.subkey)?;
    unsafe {
        set_sz(root_key.0, "", "URL:Codex Protocol")?;
        set_sz(root_key.0, "URL Protocol", "")?;
        set_sz(root_key.0, PROTOCOL_OWNER_VALUE, PROTOCOL_OWNER_ID)?;
        set_sz(
            root_key.0,
            PROTOCOL_INSTALL_ROOT_VALUE,
            &install_root.to_string_lossy(),
        )?;
    }
    let icon_key = create_key(location.hive, &location.icon_subkey())?;
    unsafe {
        set_sz(icon_key.0, "", &values.icon)?;
    }
    let command_key = create_key(location.hive, &location.command_subkey())?;
    unsafe {
        set_sz(command_key.0, "", &values.command)?;
    }
    Ok(())
}

fn create_key(hive: HKEY, subkey: &str) -> Result<RegistryKey> {
    unsafe {
        let subkey = wide(subkey);
        let mut hkey = HKEY::default();
        let mut disposition = REG_CREATE_KEY_DISPOSITION::default();
        RegCreateKeyExW(
            hive,
            PCWSTR(subkey.as_ptr()),
            0,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut hkey,
            Some(&mut disposition),
        )
        .ok()
        .context("RegCreateKeyExW")?;
        Ok(RegistryKey(hkey))
    }
}

fn key_exists(hive: HKEY, subkey: &str) -> Result<bool> {
    unsafe {
        let subkey = wide(subkey);
        let mut hkey = HKEY::default();
        let result = RegOpenKeyExW(hive, PCWSTR(subkey.as_ptr()), 0, KEY_READ, &mut hkey);
        if result == ERROR_FILE_NOT_FOUND || result == ERROR_PATH_NOT_FOUND {
            return Ok(false);
        }
        result.ok().context("RegOpenKeyExW")?;
        let _ = RegCloseKey(hkey);
        Ok(true)
    }
}

fn read_sz_at(hive: HKEY, subkey: &str, name: Option<&str>) -> Result<Option<String>> {
    unsafe {
        let subkey = wide(subkey);
        let name = name.map(wide);
        let name = name
            .as_ref()
            .map_or_else(PCWSTR::null, |value| PCWSTR(value.as_ptr()));
        let mut bytes = 0u32;
        let first = RegGetValueW(
            hive,
            PCWSTR(subkey.as_ptr()),
            name,
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut bytes),
        );
        if first == ERROR_FILE_NOT_FOUND || first == ERROR_PATH_NOT_FOUND {
            return Ok(None);
        }
        first.ok().context("RegGetValueW(size)")?;
        let mut value = vec![0u16; (bytes as usize).div_ceil(2).max(1)];
        RegGetValueW(
            hive,
            PCWSTR(subkey.as_ptr()),
            name,
            RRF_RT_REG_SZ,
            None,
            Some(value.as_mut_ptr().cast()),
            Some(&mut bytes),
        )
        .ok()
        .context("RegGetValueW(value)")?;
        let length = value
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(value.len());
        Ok(Some(String::from_utf16_lossy(&value[..length])))
    }
}

fn delete_tree(hive: HKEY, subkey: &str) -> Result<()> {
    unsafe {
        let subkey = wide(subkey);
        let result = RegDeleteTreeW(hive, PCWSTR(subkey.as_ptr()));
        if result == ERROR_FILE_NOT_FOUND || result == ERROR_PATH_NOT_FOUND {
            return Ok(());
        }
        result.ok().context("RegDeleteTreeW")
    }
}

fn effective_protocol_handler(scheme: &str) -> Result<Option<PathBuf>> {
    unsafe {
        let scheme = wide(scheme);
        let verb = wide("open");
        let mut length = 0u32;
        let first = AssocQueryStringW(
            ASSOCF_NONE,
            ASSOCSTR_EXECUTABLE,
            PCWSTR(scheme.as_ptr()),
            PCWSTR(verb.as_ptr()),
            PWSTR::null(),
            &mut length,
        );
        if length == 0 {
            if first == HRESULT::from_win32(ERROR_NO_ASSOCIATION.0)
                || first == HRESULT::from_win32(ERROR_FILE_NOT_FOUND.0)
                || first == HRESULT::from_win32(ERROR_PATH_NOT_FOUND.0)
            {
                return Ok(None);
            }
            first.ok().context("AssocQueryStringW(size)")?;
        }
        let mut value = vec![0u16; length.max(1) as usize];
        AssocQueryStringW(
            ASSOCF_NONE,
            ASSOCSTR_EXECUTABLE,
            PCWSTR(scheme.as_ptr()),
            PCWSTR(verb.as_ptr()),
            PWSTR(value.as_mut_ptr()),
            &mut length,
        )
        .ok()
        .context("AssocQueryStringW(value)")?;
        let length = value
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(value.len());
        Ok(Some(PathBuf::from(String::from_utf16_lossy(
            &value[..length],
        ))))
    }
}

fn notify_association_changed() {
    unsafe {
        SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None);
    }
}

fn classify_protocol(
    existing: &ExistingProtocol,
    install_root: &Path,
    desired: &ProtocolValues,
) -> CodexProtocolStatus {
    if !existing.key_exists {
        return CodexProtocolStatus::Missing;
    }
    if existing
        .owner
        .as_deref()
        .is_some_and(|owner| owner != PROTOCOL_OWNER_ID)
    {
        return CodexProtocolStatus::OtherOwner;
    }
    if existing
        .install_root
        .as_deref()
        .is_some_and(|root| !paths_equal(Path::new(root), install_root))
    {
        return CodexProtocolStatus::OtherOwner;
    }

    let owned_command = existing
        .command
        .as_deref()
        .and_then(protocol_command_executable)
        .is_some_and(|executable| protocol_handler_belongs_to(install_root, &executable));
    if !owned_command {
        let marker_owned = existing.owner.as_deref() == Some(PROTOCOL_OWNER_ID)
            && existing
                .install_root
                .as_deref()
                .is_some_and(|root| paths_equal(Path::new(root), install_root));
        return if existing.command.is_none() && marker_owned {
            CodexProtocolStatus::NeedsRepair
        } else {
            CodexProtocolStatus::OtherOwner
        };
    }

    let markers_ready = existing.owner.as_deref() == Some(PROTOCOL_OWNER_ID)
        && existing
            .install_root
            .as_deref()
            .is_some_and(|root| paths_equal(Path::new(root), install_root));
    if markers_ready && existing.command.as_deref() == Some(desired.command.as_str()) {
        CodexProtocolStatus::Ready
    } else {
        CodexProtocolStatus::NeedsRepair
    }
}

fn protocol_command_executable(command: &str) -> Option<std::path::PathBuf> {
    let command = command.trim();
    let quoted = command.strip_prefix('"')?;
    let end = quoted.find('"')?;
    if quoted[end + 1..].trim() != r#""%1""# {
        return None;
    }
    Some(std::path::PathBuf::from(&quoted[..end]))
}

fn protocol_handler_belongs_to(install_root: &Path, executable: &Path) -> bool {
    let root = normalized_path(install_root);
    let executable = normalized_path(executable);
    let Some(relative) = executable.strip_prefix(&(root + r"\versions\")) else {
        return false;
    };
    let mut parts = relative.split('\\');
    let version = parts.next().unwrap_or_default();
    let file_name = parts.next().unwrap_or_default();
    !version.is_empty()
        && parts.next().is_none()
        && matches!(file_name, "chatgpt.exe" | "codex.exe")
}

fn paths_equal(left: &Path, right: &Path) -> bool {
    normalized_path(left) == normalized_path(right)
}

fn normalized_path(path: &Path) -> String {
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    path.to_string_lossy()
        .replace('/', "\\")
        .trim_start_matches(r"\\?\")
        .trim_end_matches('\\')
        .to_ascii_lowercase()
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_PROTOCOL_TEST: AtomicU64 = AtomicU64::new(1);

    struct TestProtocolFixture {
        root: std::path::PathBuf,
        handler: std::path::PathBuf,
        location: ProtocolRegistryLocation,
    }

    impl TestProtocolFixture {
        fn new() -> Self {
            let id = NEXT_PROTOCOL_TEST.fetch_add(1, Ordering::Relaxed);
            let scheme = format!("codex-windows-cn-test-{}-{id}", std::process::id());
            let root = std::env::temp_dir().join(format!(
                "codex protocol 中文 test-{}-{id}",
                std::process::id()
            ));
            let handler = root.join("versions").join("current").join("ChatGPT.exe");
            std::fs::create_dir_all(handler.parent().expect("handler parent"))
                .expect("create protocol test install");
            std::fs::write(&handler, b"test handler").expect("write protocol test handler");
            Self {
                root,
                handler,
                location: ProtocolRegistryLocation::new(HKEY_CURRENT_USER, &scheme),
            }
        }
    }

    impl Drop for TestProtocolFixture {
        fn drop(&mut self) {
            let subkey = wide(&self.location.subkey);
            unsafe {
                let _ = RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(subkey.as_ptr()));
            }
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn protocol_values_quote_executable_and_uri_placeholder() {
        let executable = Path::new(r"D:\Codex 中文版\versions\current\ChatGPT.exe");

        let values = ProtocolValues::new(executable);

        assert_eq!(
            values.command,
            r#""D:\Codex 中文版\versions\current\ChatGPT.exe" "%1""#
        );
        assert_eq!(
            values.icon,
            r#""D:\Codex 中文版\versions\current\ChatGPT.exe",0"#
        );
    }

    #[test]
    fn legacy_handler_is_repairable_but_similar_path_is_foreign() {
        let root = Path::new(r"D:\Codex");
        let desired = ProtocolValues::new(Path::new(r"D:\Codex\versions\current\ChatGPT.exe"));
        let legacy = ExistingProtocol {
            key_exists: true,
            command: Some(r#""D:\Codex\versions\26.7.1\ChatGPT.exe" "%1""#.into()),
            ..ExistingProtocol::default()
        };
        let foreign = ExistingProtocol {
            key_exists: true,
            command: Some(r#""D:\Codex-other\versions\26.7.1\ChatGPT.exe" "%1""#.into()),
            ..ExistingProtocol::default()
        };

        assert_eq!(
            classify_protocol(&legacy, root, &desired),
            CodexProtocolStatus::NeedsRepair
        );
        assert_eq!(
            classify_protocol(&foreign, root, &desired),
            CodexProtocolStatus::OtherOwner
        );
    }

    #[test]
    fn isolated_protocol_registration_round_trip_is_idempotent() {
        let fixture = TestProtocolFixture::new();

        assert_eq!(
            register_codex_protocol_at(&fixture.location, &fixture.root, &fixture.handler, false,)
                .expect("register protocol"),
            ProtocolRegistration::Created
        );
        assert_eq!(
            codex_protocol_status_at(&fixture.location, &fixture.root, &fixture.handler)
                .expect("inspect protocol"),
            CodexProtocolStatus::Ready
        );
        assert_eq!(
            read_sz_at(
                fixture.location.hive,
                &fixture.location.command_subkey(),
                None,
            )
            .expect("read command")
            .as_deref(),
            Some(ProtocolValues::new(&fixture.handler).command.as_str())
        );
        assert_eq!(
            register_codex_protocol_at(&fixture.location, &fixture.root, &fixture.handler, false,)
                .expect("register protocol again"),
            ProtocolRegistration::Unchanged
        );
        assert_eq!(
            remove_codex_protocol_at(&fixture.location, &fixture.root).expect("remove protocol"),
            ProtocolRemoval::Removed
        );
        assert_eq!(
            remove_codex_protocol_at(&fixture.location, &fixture.root)
                .expect("remove missing protocol"),
            ProtocolRemoval::NotFound
        );
    }

    #[test]
    fn foreign_protocol_is_preserved_until_user_explicitly_replaces_it() {
        let fixture = TestProtocolFixture::new();
        let foreign_command = r#""D:\Other Codex\versions\current\ChatGPT.exe" "%1""#;
        let command_key = create_key(fixture.location.hive, &fixture.location.command_subkey())
            .expect("create foreign command key");
        unsafe {
            set_sz(command_key.0, "", foreign_command).expect("write foreign command");
        }

        assert_eq!(
            register_codex_protocol_at(&fixture.location, &fixture.root, &fixture.handler, false,)
                .expect("preserve foreign protocol"),
            ProtocolRegistration::PreservedForeign
        );
        assert_eq!(
            remove_codex_protocol_at(&fixture.location, &fixture.root)
                .expect("preserve foreign protocol on remove"),
            ProtocolRemoval::PreservedForeign
        );
        assert_eq!(
            read_sz_at(
                fixture.location.hive,
                &fixture.location.command_subkey(),
                None,
            )
            .expect("read preserved command")
            .as_deref(),
            Some(foreign_command)
        );

        assert_eq!(
            register_codex_protocol_at(&fixture.location, &fixture.root, &fixture.handler, true,)
                .expect("replace foreign protocol explicitly"),
            ProtocolRegistration::Refreshed
        );
        assert_eq!(
            codex_protocol_status_at(&fixture.location, &fixture.root, &fixture.handler)
                .expect("inspect replaced protocol"),
            CodexProtocolStatus::Ready
        );
    }

    #[test]
    fn missing_target_key_does_not_shadow_an_effective_foreign_handler() {
        let fixture = TestProtocolFixture::new();
        let foreign_handler = fixture.root.join("foreign").join("ChatGPT.exe");
        std::fs::create_dir_all(foreign_handler.parent().expect("foreign handler parent"))
            .expect("create foreign handler directory");
        std::fs::write(&foreign_handler, b"foreign handler").expect("write foreign handler");
        let effective_root = create_key(fixture.location.hive, &fixture.location.subkey)
            .expect("create effective protocol root");
        unsafe {
            set_sz(effective_root.0, "", "URL:Foreign Protocol")
                .expect("write foreign protocol name");
            set_sz(effective_root.0, "URL Protocol", "").expect("mark URL protocol");
        }
        let effective_command =
            create_key(fixture.location.hive, &fixture.location.command_subkey())
                .expect("create effective foreign command");
        unsafe {
            set_sz(
                effective_command.0,
                "",
                &ProtocolValues::new(&foreign_handler).command,
            )
            .expect("write effective foreign command");
        }
        let shadow_location = ProtocolRegistryLocation {
            hive: fixture.location.hive,
            scheme: fixture.location.scheme.clone(),
            subkey: format!(r"{}\shadow-target", fixture.location.subkey),
        };

        assert_eq!(
            register_codex_protocol_at(&shadow_location, &fixture.root, &fixture.handler, false,)
                .expect("preserve effective foreign handler"),
            ProtocolRegistration::PreservedForeign
        );
        assert!(!key_exists(shadow_location.hive, &shadow_location.subkey)
            .expect("inspect missing shadow target"));
    }
}

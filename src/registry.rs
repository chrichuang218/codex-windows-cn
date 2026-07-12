//! Add/Remove Programs registration — writes our `Uninstall\<key>` so
//! Windows Settings → Apps lists us and runs our `--uninstall`. Key name
//! is unique to this project to avoid colliding with anything OpenAI ships.
//!
//! User-mode installs write to HKCU (no admin needed). System-mode writes
//! to HKLM and therefore requires elevation — caller must already be
//! running elevated.

use crate::config::InstallMode;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use windows::core::{HRESULT, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CO_E_APPNOTFOUND, ERROR_FILE_NOT_FOUND, ERROR_NO_ASSOCIATION, ERROR_PATH_NOT_FOUND,
};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegDeleteValueW, RegGetValueW, RegOpenKeyExW,
    RegSetValueExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WRITE,
    REG_CREATE_KEY_DISPOSITION, REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ,
};
use windows::Win32::UI::Shell::{
    AssocQueryStringW, SHChangeNotify, ASSOCF_NONE, ASSOCSTR_EXECUTABLE, SHCNE_ASSOCCHANGED,
    SHCNF_FLUSH, SHCNF_IDLIST,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProtocolOwnership {
    Missing,
    Owned,
    Foreign,
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
    url_protocol: Option<String>,
    owner: Option<String>,
    install_root: Option<String>,
    command: Option<String>,
    delegate_execute: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
struct ProtocolSnapshot {
    root_key_exists: bool,
    icon_key_exists: bool,
    shell_key_exists: bool,
    open_key_exists: bool,
    command_key_exists: bool,
    display_name: Option<String>,
    url_protocol: Option<String>,
    owner: Option<String>,
    install_root: Option<String>,
    icon: Option<String>,
    command: Option<String>,
    delegate_execute: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct CodexProtocolBackup {
    snapshot: ProtocolSnapshot,
}

struct ProtocolRegistryLocation {
    hive: HKEY,
    scheme: String,
    subkey: String,
    scope: ProtocolRegistryScope,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ProtocolRegistryScope {
    CurrentUser,
    Machine,
}

impl ProtocolRegistryLocation {
    fn new(hive: HKEY, scheme: &str) -> Self {
        Self {
            hive,
            scheme: scheme.to_string(),
            subkey: format!(r"Software\Classes\{scheme}"),
            scope: if hive == HKEY_LOCAL_MACHINE {
                ProtocolRegistryScope::Machine
            } else {
                ProtocolRegistryScope::CurrentUser
            },
        }
    }

    fn icon_subkey(&self) -> String {
        format!(r"{}\DefaultIcon", self.subkey)
    }

    fn command_subkey(&self) -> String {
        format!(r"{}\shell\open\command", self.subkey)
    }

    fn shell_subkey(&self) -> String {
        format!(r"{}\shell", self.subkey)
    }

    fn open_subkey(&self) -> String {
        format!(r"{}\shell\open", self.subkey)
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

pub fn register_codex_protocol_with_backup(
    mode: InstallMode,
    install_root: &Path,
    handler_exe: &Path,
) -> Result<(ProtocolRegistration, CodexProtocolBackup)> {
    let (registration, snapshot) = register_codex_protocol_at_with_snapshot(
        &ProtocolRegistryLocation::new(root(mode), "codex"),
        install_root,
        handler_exe,
        false,
    )?;
    Ok((registration, CodexProtocolBackup { snapshot }))
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

pub fn replace_codex_protocol_with_backup(
    mode: InstallMode,
    install_root: &Path,
    handler_exe: &Path,
) -> Result<(ProtocolRegistration, CodexProtocolBackup)> {
    let (registration, snapshot) = register_codex_protocol_at_with_snapshot(
        &ProtocolRegistryLocation::new(root(mode), "codex"),
        install_root,
        handler_exe,
        true,
    )?;
    Ok((registration, CodexProtocolBackup { snapshot }))
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

pub fn remove_codex_protocol_if_owned_with_backup(
    mode: InstallMode,
    install_root: &Path,
) -> Result<(ProtocolRemoval, CodexProtocolBackup)> {
    let (removal, snapshot) = remove_codex_protocol_at_with_snapshot(
        &ProtocolRegistryLocation::new(root(mode), "codex"),
        install_root,
    )?;
    Ok((removal, CodexProtocolBackup { snapshot }))
}

pub fn capture_codex_protocol(mode: InstallMode) -> Result<CodexProtocolBackup> {
    Ok(CodexProtocolBackup {
        snapshot: read_protocol_snapshot(&ProtocolRegistryLocation::new(root(mode), "codex"))?,
    })
}

pub fn restore_codex_protocol(mode: InstallMode, backup: &CodexProtocolBackup) -> Result<()> {
    restore_protocol_snapshot(
        &ProtocolRegistryLocation::new(root(mode), "codex"),
        &backup.snapshot,
    )?;
    notify_association_changed();
    Ok(())
}

pub fn restore_codex_protocol_if_unchanged(
    mode: InstallMode,
    expected: &CodexProtocolBackup,
    backup: &CodexProtocolBackup,
) -> Result<bool> {
    let location = ProtocolRegistryLocation::new(root(mode), "codex");
    let current = CodexProtocolBackup {
        snapshot: read_protocol_snapshot(&location)?,
    };
    if current != *expected {
        return Ok(false);
    }
    restore_protocol_snapshot(&location, &backup.snapshot)?;
    notify_association_changed();
    Ok(true)
}

pub fn system_codex_protocol_replace_is_safe_for_current_user(install_root: &Path) -> Result<bool> {
    if current_user_protocol_override_exists("codex")? {
        return Ok(false);
    }
    let location = ProtocolRegistryLocation::new(HKEY_LOCAL_MACHINE, "codex");
    let existing = read_existing_protocol(&location)?;
    let effective_handler = effective_protocol_handler(&location.scheme)?;
    Ok(target_scope_can_replace_effective_handler(
        &existing,
        install_root,
        effective_handler.as_deref(),
    ))
}

fn target_scope_can_replace_effective_handler(
    existing: &ExistingProtocol,
    install_root: &Path,
    effective_handler: Option<&Path>,
) -> bool {
    if existing.delegate_execute.is_some() {
        return true;
    }
    match effective_handler {
        None => true,
        Some(handler) if protocol_handler_belongs_to(install_root, handler) => true,
        Some(handler) => existing
            .command
            .as_deref()
            .and_then(protocol_command_executable)
            .is_some_and(|target_handler| paths_equal(&target_handler, handler)),
    }
}

fn current_user_protocol_override_exists(scheme: &str) -> Result<bool> {
    let classes_subkey = format!(r"Software\Classes\{scheme}");
    let user_choice_subkey = format!(
        r"Software\Microsoft\Windows\Shell\Associations\UrlAssociations\{scheme}\UserChoice"
    );
    Ok(key_exists(HKEY_CURRENT_USER, &classes_subkey)?
        || key_exists(HKEY_CURRENT_USER, &user_choice_subkey)?)
}

fn codex_protocol_status_at(
    location: &ProtocolRegistryLocation,
    install_root: &Path,
    handler_exe: &Path,
) -> Result<CodexProtocolStatus> {
    validate_protocol_handler(install_root, handler_exe)?;
    inspect_protocol_status(location, install_root, handler_exe)
}

fn inspect_protocol_status(
    location: &ProtocolRegistryLocation,
    install_root: &Path,
    handler_exe: &Path,
) -> Result<CodexProtocolStatus> {
    let existing = read_existing_protocol(location)?;
    let effective_handler = effective_protocol_handler(&location.scheme)?;
    Ok(classify_protocol_with_effective(
        &existing,
        install_root,
        &ProtocolValues::new(handler_exe),
        handler_exe,
        effective_handler.as_deref(),
    ))
}

fn register_codex_protocol_at(
    location: &ProtocolRegistryLocation,
    install_root: &Path,
    handler_exe: &Path,
    replace_foreign: bool,
) -> Result<ProtocolRegistration> {
    register_codex_protocol_at_with_snapshot(location, install_root, handler_exe, replace_foreign)
        .map(|(registration, _)| registration)
}

fn register_codex_protocol_at_with_snapshot(
    location: &ProtocolRegistryLocation,
    install_root: &Path,
    handler_exe: &Path,
    replace_foreign: bool,
) -> Result<(ProtocolRegistration, ProtocolSnapshot)> {
    validate_protocol_handler(install_root, handler_exe)?;
    let desired = ProtocolValues::new(handler_exe);
    let snapshot = read_protocol_snapshot(location)?;
    let existing = read_existing_protocol(location)?;
    let effective_handler = effective_protocol_handler(&location.scheme)?;
    let status = classify_protocol_with_effective(
        &existing,
        install_root,
        &desired,
        handler_exe,
        effective_handler.as_deref(),
    );
    let target_status = classify_protocol(&existing, install_root, &desired);
    let maintain_owned_target = !replace_foreign
        && matches!(
            target_status,
            CodexProtocolStatus::Ready | CodexProtocolStatus::NeedsRepair
        );
    let registration_status =
        if location.scope == ProtocolRegistryScope::Machine || maintain_owned_target {
            target_status
        } else {
            status
        };
    if registration_status == CodexProtocolStatus::Ready {
        return Ok((ProtocolRegistration::Unchanged, snapshot));
    }
    if registration_status == CodexProtocolStatus::OtherOwner && !replace_foreign {
        return Ok((ProtocolRegistration::PreservedForeign, snapshot));
    }

    if let Err(cause) = write_protocol(location, install_root, &desired) {
        return Err(rollback_protocol_change(
            location,
            install_root,
            &snapshot,
            None,
            cause,
        ));
    }
    let written_snapshot = match read_protocol_snapshot(location) {
        Ok(snapshot) => snapshot,
        Err(cause) => {
            return Err(rollback_protocol_change(
                location,
                install_root,
                &snapshot,
                None,
                cause.context("capture codex:// written state"),
            ));
        }
    };
    notify_association_changed();

    let verify_effective =
        location.scope == ProtocolRegistryScope::CurrentUser && !maintain_owned_target;
    let verified =
        match verify_protocol_registration(location, install_root, handler_exe, verify_effective) {
            Ok(status) => status,
            Err(cause) => {
                return Err(rollback_protocol_change(
                    location,
                    install_root,
                    &snapshot,
                    Some(&written_snapshot),
                    cause.context("verify codex:// protocol registration"),
                ));
            }
        };
    if verified != CodexProtocolStatus::Ready {
        return Err(rollback_protocol_change(
            location,
            install_root,
            &snapshot,
            Some(&written_snapshot),
            anyhow::anyhow!(
                "codex:// registration did not become effective; current status: {verified:?}"
            ),
        ));
    }

    Ok((
        if snapshot.root_key_exists {
            ProtocolRegistration::Refreshed
        } else {
            ProtocolRegistration::Created
        },
        written_snapshot,
    ))
}

fn verify_protocol_registration(
    location: &ProtocolRegistryLocation,
    install_root: &Path,
    handler_exe: &Path,
    verify_effective: bool,
) -> Result<CodexProtocolStatus> {
    if !verify_effective {
        return Ok(classify_protocol(
            &read_existing_protocol(location)?,
            install_root,
            &ProtocolValues::new(handler_exe),
        ));
    }
    inspect_protocol_status(location, install_root, handler_exe)
}

fn remove_codex_protocol_at(
    location: &ProtocolRegistryLocation,
    install_root: &Path,
) -> Result<ProtocolRemoval> {
    remove_codex_protocol_at_with_snapshot(location, install_root).map(|(removal, _)| removal)
}

fn remove_codex_protocol_at_with_snapshot(
    location: &ProtocolRegistryLocation,
    install_root: &Path,
) -> Result<(ProtocolRemoval, ProtocolSnapshot)> {
    let snapshot = read_protocol_snapshot(location)?;
    let existing = read_existing_protocol(location)?;
    match protocol_ownership(&existing, install_root) {
        ProtocolOwnership::Missing => return Ok((ProtocolRemoval::NotFound, snapshot)),
        ProtocolOwnership::Foreign => return Ok((ProtocolRemoval::PreservedForeign, snapshot)),
        ProtocolOwnership::Owned => {}
    }
    delete_tree(location.hive, &location.subkey)?;
    notify_association_changed();
    Ok((ProtocolRemoval::Removed, ProtocolSnapshot::default()))
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
        url_protocol: read_sz_at(location.hive, &location.subkey, Some("URL Protocol"))?,
        owner: read_sz_at(location.hive, &location.subkey, Some(PROTOCOL_OWNER_VALUE))?,
        install_root: read_sz_at(
            location.hive,
            &location.subkey,
            Some(PROTOCOL_INSTALL_ROOT_VALUE),
        )?,
        command: read_sz_at(location.hive, &location.command_subkey(), None)?,
        delegate_execute: read_sz_at(
            location.hive,
            &location.command_subkey(),
            Some("DelegateExecute"),
        )?,
    })
}

fn read_protocol_snapshot(location: &ProtocolRegistryLocation) -> Result<ProtocolSnapshot> {
    let root_key_exists = key_exists(location.hive, &location.subkey)?;
    if !root_key_exists {
        return Ok(ProtocolSnapshot::default());
    }

    let icon_subkey = location.icon_subkey();
    let shell_subkey = location.shell_subkey();
    let open_subkey = location.open_subkey();
    let command_subkey = location.command_subkey();
    Ok(ProtocolSnapshot {
        root_key_exists,
        icon_key_exists: key_exists(location.hive, &icon_subkey)?,
        shell_key_exists: key_exists(location.hive, &shell_subkey)?,
        open_key_exists: key_exists(location.hive, &open_subkey)?,
        command_key_exists: key_exists(location.hive, &command_subkey)?,
        display_name: read_sz_at(location.hive, &location.subkey, None)?,
        url_protocol: read_sz_at(location.hive, &location.subkey, Some("URL Protocol"))?,
        owner: read_sz_at(location.hive, &location.subkey, Some(PROTOCOL_OWNER_VALUE))?,
        install_root: read_sz_at(
            location.hive,
            &location.subkey,
            Some(PROTOCOL_INSTALL_ROOT_VALUE),
        )?,
        icon: read_sz_at(location.hive, &icon_subkey, None)?,
        command: read_sz_at(location.hive, &command_subkey, None)?,
        delegate_execute: read_sz_at(location.hive, &command_subkey, Some("DelegateExecute"))?,
    })
}

fn rollback_protocol_change(
    location: &ProtocolRegistryLocation,
    install_root: &Path,
    snapshot: &ProtocolSnapshot,
    expected: Option<&ProtocolSnapshot>,
    cause: anyhow::Error,
) -> anyhow::Error {
    let current = match read_protocol_snapshot(location) {
        Ok(current) => current,
        Err(inspect) => {
            return anyhow::anyhow!(
                "{cause:#}; inspecting codex:// before rollback also failed: {inspect:#}"
            );
        }
    };
    let safe_to_restore = match expected {
        Some(expected) => current == *expected,
        None if current == *snapshot => true,
        None => read_existing_protocol(location)
            .map(|existing| protocol_ownership(&existing, install_root) == ProtocolOwnership::Owned)
            .unwrap_or(false),
    };
    if !safe_to_restore {
        return anyhow::anyhow!(
            "{cause:#}; codex:// changed concurrently, so the previous registration was not restored"
        );
    }
    match restore_protocol_snapshot(location, snapshot) {
        Ok(()) => {
            notify_association_changed();
            cause
        }
        Err(rollback) => anyhow::anyhow!(
            "{cause:#}; restoring the previous codex:// registration also failed: {rollback:#}"
        ),
    }
}

fn restore_protocol_snapshot(
    location: &ProtocolRegistryLocation,
    snapshot: &ProtocolSnapshot,
) -> Result<()> {
    if !snapshot.root_key_exists {
        return delete_tree(location.hive, &location.subkey);
    }

    let root_key = create_key(location.hive, &location.subkey)?;
    unsafe {
        restore_sz(root_key.0, "", snapshot.display_name.as_deref())?;
        restore_sz(root_key.0, "URL Protocol", snapshot.url_protocol.as_deref())?;
        restore_sz(root_key.0, PROTOCOL_OWNER_VALUE, snapshot.owner.as_deref())?;
        restore_sz(
            root_key.0,
            PROTOCOL_INSTALL_ROOT_VALUE,
            snapshot.install_root.as_deref(),
        )?;
    }

    let icon_subkey = location.icon_subkey();
    if snapshot.icon_key_exists {
        let icon_key = create_key(location.hive, &icon_subkey)?;
        unsafe {
            restore_sz(icon_key.0, "", snapshot.icon.as_deref())?;
        }
    } else {
        delete_tree(location.hive, &icon_subkey)?;
    }

    let command_subkey = location.command_subkey();
    if snapshot.command_key_exists {
        let command_key = create_key(location.hive, &command_subkey)?;
        unsafe {
            restore_sz(command_key.0, "", snapshot.command.as_deref())?;
            restore_sz(
                command_key.0,
                "DelegateExecute",
                snapshot.delegate_execute.as_deref(),
            )?;
        }
    } else {
        delete_tree(location.hive, &command_subkey)?;
    }
    if !snapshot.open_key_exists {
        delete_tree(location.hive, &location.open_subkey())?;
    }
    if !snapshot.shell_key_exists {
        delete_tree(location.hive, &location.shell_subkey())?;
    }
    Ok(())
}

fn protocol_ownership(existing: &ExistingProtocol, install_root: &Path) -> ProtocolOwnership {
    if !existing.key_exists {
        return ProtocolOwnership::Missing;
    }
    if existing.delegate_execute.is_some() {
        return ProtocolOwnership::Foreign;
    }
    if existing
        .owner
        .as_deref()
        .is_some_and(|owner| owner != PROTOCOL_OWNER_ID)
        || existing
            .install_root
            .as_deref()
            .is_some_and(|root| !paths_equal(Path::new(root), install_root))
    {
        return ProtocolOwnership::Foreign;
    }
    let marker_owned = existing.owner.as_deref() == Some(PROTOCOL_OWNER_ID)
        && existing
            .install_root
            .as_deref()
            .is_some_and(|root| paths_equal(Path::new(root), install_root));
    let command_owned = existing
        .command
        .as_deref()
        .and_then(protocol_command_executable)
        .is_some_and(|executable| protocol_handler_belongs_to(install_root, &executable));
    match existing.command.as_ref() {
        Some(_) if command_owned => ProtocolOwnership::Owned,
        Some(_) => ProtocolOwnership::Foreign,
        None if marker_owned => ProtocolOwnership::Owned,
        None => ProtocolOwnership::Foreign,
    }
}

fn write_protocol(
    location: &ProtocolRegistryLocation,
    install_root: &Path,
    values: &ProtocolValues,
) -> Result<()> {
    let command_key = create_key(location.hive, &location.command_subkey())?;
    unsafe {
        delete_value(command_key.0, "DelegateExecute")?;
        set_sz(command_key.0, "", &values.command)?;
    }
    let icon_key = create_key(location.hive, &location.icon_subkey())?;
    unsafe {
        set_sz(icon_key.0, "", &values.icon)?;
    }
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
                || first == CO_E_APPNOTFOUND
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
        SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST | SHCNF_FLUSH, None, None);
    }
}

fn classify_protocol(
    existing: &ExistingProtocol,
    install_root: &Path,
    desired: &ProtocolValues,
) -> CodexProtocolStatus {
    match protocol_ownership(existing, install_root) {
        ProtocolOwnership::Missing => return CodexProtocolStatus::Missing,
        ProtocolOwnership::Foreign => return CodexProtocolStatus::OtherOwner,
        ProtocolOwnership::Owned => {}
    }

    let markers_ready = existing.owner.as_deref() == Some(PROTOCOL_OWNER_ID)
        && existing
            .install_root
            .as_deref()
            .is_some_and(|root| paths_equal(Path::new(root), install_root));
    if existing.url_protocol.as_deref() == Some("")
        && markers_ready
        && existing.command.as_deref() == Some(desired.command.as_str())
        && existing.delegate_execute.is_none()
    {
        CodexProtocolStatus::Ready
    } else {
        CodexProtocolStatus::NeedsRepair
    }
}

fn classify_protocol_with_effective(
    existing: &ExistingProtocol,
    install_root: &Path,
    desired: &ProtocolValues,
    desired_handler: &Path,
    effective_handler: Option<&Path>,
) -> CodexProtocolStatus {
    let stored_status = classify_protocol(existing, install_root, desired);
    if stored_status == CodexProtocolStatus::OtherOwner {
        return CodexProtocolStatus::OtherOwner;
    }
    match effective_handler {
        Some(handler) if !protocol_handler_belongs_to(install_root, handler) => {
            CodexProtocolStatus::OtherOwner
        }
        Some(handler) if !paths_equal(handler, desired_handler) => CodexProtocolStatus::NeedsRepair,
        None if stored_status == CodexProtocolStatus::Ready => CodexProtocolStatus::NeedsRepair,
        _ => stored_status,
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

unsafe fn restore_sz(hkey: HKEY, name: &str, value: Option<&str>) -> Result<()> {
    match value {
        Some(value) => set_sz(hkey, name, value),
        None => delete_value(hkey, name),
    }
}

unsafe fn delete_value(hkey: HKEY, name: &str) -> Result<()> {
    let name_w = wide(name);
    let result = RegDeleteValueW(hkey, PCWSTR(name_w.as_ptr()));
    if result == ERROR_FILE_NOT_FOUND || result == ERROR_PATH_NOT_FOUND {
        return Ok(());
    }
    result
        .ok()
        .with_context(|| format!("RegDeleteValueW({name})"))
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

        fn shadow_location(&self) -> ProtocolRegistryLocation {
            ProtocolRegistryLocation {
                hive: self.location.hive,
                scheme: self.location.scheme.clone(),
                subkey: format!(r"{}\shadow-target", self.location.subkey),
                scope: ProtocolRegistryScope::CurrentUser,
            }
        }

        fn machine_shadow_location(&self) -> ProtocolRegistryLocation {
            ProtocolRegistryLocation {
                hive: self.location.hive,
                scheme: self.location.scheme.clone(),
                subkey: format!(r"{}\machine-shadow-target", self.location.subkey),
                scope: ProtocolRegistryScope::Machine,
            }
        }

        fn write_effective_foreign_handler(&self) -> PathBuf {
            let handler = self.root.join("foreign").join("ChatGPT.exe");
            std::fs::create_dir_all(handler.parent().expect("foreign handler parent"))
                .expect("create foreign handler directory");
            std::fs::write(&handler, b"foreign handler").expect("write foreign handler");
            write_protocol(
                &self.location,
                &self.root.join("foreign-install"),
                &ProtocolValues::new(&handler),
            )
            .expect("write effective foreign protocol");
            notify_association_changed();
            handler
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
    fn stale_owned_markers_do_not_claim_a_foreign_command() {
        let root = Path::new(r"D:\Codex");
        let desired = ProtocolValues::new(Path::new(r"D:\Codex\versions\current\ChatGPT.exe"));
        let existing = ExistingProtocol {
            key_exists: true,
            url_protocol: Some(String::new()),
            owner: Some(PROTOCOL_OWNER_ID.into()),
            install_root: Some(root.to_string_lossy().into_owned()),
            command: Some(r#""D:\Other Codex\ChatGPT.exe" "%1""#.into()),
            delegate_execute: None,
        };

        assert_eq!(
            classify_protocol(&existing, root, &desired),
            CodexProtocolStatus::OtherOwner
        );
    }

    #[test]
    fn missing_url_protocol_marker_needs_repair() {
        let root = Path::new(r"D:\Codex");
        let desired = ProtocolValues::new(Path::new(r"D:\Codex\versions\current\ChatGPT.exe"));
        let existing = ExistingProtocol {
            key_exists: true,
            url_protocol: None,
            owner: Some(PROTOCOL_OWNER_ID.into()),
            install_root: Some(root.to_string_lossy().into_owned()),
            command: Some(desired.command.clone()),
            delegate_execute: None,
        };

        assert_eq!(
            classify_protocol(&existing, root, &desired),
            CodexProtocolStatus::NeedsRepair
        );
    }

    #[test]
    fn delegate_execute_is_foreign_even_with_owned_command() {
        let root = Path::new(r"D:\Codex");
        let desired = ProtocolValues::new(Path::new(r"D:\Codex\versions\current\ChatGPT.exe"));
        let existing = ExistingProtocol {
            key_exists: true,
            url_protocol: Some(String::new()),
            owner: Some(PROTOCOL_OWNER_ID.into()),
            install_root: Some(root.to_string_lossy().into_owned()),
            command: Some(desired.command.clone()),
            delegate_execute: Some("{foreign-handler}".into()),
        };

        assert_eq!(
            classify_protocol(&existing, root, &desired),
            CodexProtocolStatus::OtherOwner
        );
    }

    #[test]
    fn stale_owned_markers_do_not_claim_a_delegate_only_handler() {
        let root = Path::new(r"D:\Codex");
        let desired = ProtocolValues::new(Path::new(r"D:\Codex\versions\current\ChatGPT.exe"));
        let existing = ExistingProtocol {
            key_exists: true,
            url_protocol: Some(String::new()),
            owner: Some(PROTOCOL_OWNER_ID.into()),
            install_root: Some(root.to_string_lossy().into_owned()),
            command: None,
            delegate_execute: Some("{foreign-handler}".into()),
        };

        assert_eq!(
            classify_protocol(&existing, root, &desired),
            CodexProtocolStatus::OtherOwner
        );
    }

    #[test]
    fn effective_foreign_handler_overrides_ready_target_registration() {
        let root = Path::new(r"D:\Codex");
        let handler = Path::new(r"D:\Codex\versions\current\ChatGPT.exe");
        let desired = ProtocolValues::new(handler);
        let existing = ExistingProtocol {
            key_exists: true,
            url_protocol: Some(String::new()),
            owner: Some(PROTOCOL_OWNER_ID.into()),
            install_root: Some(root.to_string_lossy().into_owned()),
            command: Some(desired.command.clone()),
            delegate_execute: None,
        };

        assert_eq!(
            classify_protocol_with_effective(
                &existing,
                root,
                &desired,
                handler,
                Some(Path::new(r"D:\Other Codex\ChatGPT.exe")),
            ),
            CodexProtocolStatus::OtherOwner
        );
    }

    #[test]
    fn system_replace_preflight_rejects_handler_outside_target_scope() {
        let existing = ExistingProtocol {
            key_exists: true,
            command: Some(r#""D:\System Codex\ChatGPT.exe" "%1""#.into()),
            ..ExistingProtocol::default()
        };

        assert!(target_scope_can_replace_effective_handler(
            &existing,
            Path::new(r"D:\Managed Codex"),
            Some(Path::new(r"D:\System Codex\ChatGPT.exe")),
        ));
        assert!(!target_scope_can_replace_effective_handler(
            &existing,
            Path::new(r"D:\Managed Codex"),
            Some(Path::new(r"D:\Current User Codex\ChatGPT.exe")),
        ));

        let delegated = ExistingProtocol {
            key_exists: true,
            delegate_execute: Some("{machine-handler}".into()),
            ..ExistingProtocol::default()
        };
        assert!(target_scope_can_replace_effective_handler(
            &delegated,
            Path::new(r"D:\Managed Codex"),
            Some(Path::new(r"D:\Delegate Host\Handler.exe")),
        ));
    }

    #[test]
    fn isolated_protocol_registration_round_trip_is_idempotent() {
        let fixture = TestProtocolFixture::new();

        assert!(
            !current_user_protocol_override_exists(&fixture.location.scheme)
                .expect("inspect missing current-user override")
        );

        assert_eq!(
            register_codex_protocol_at(&fixture.location, &fixture.root, &fixture.handler, false,)
                .expect("register protocol"),
            ProtocolRegistration::Created
        );
        assert!(
            current_user_protocol_override_exists(&fixture.location.scheme)
                .expect("inspect current-user override")
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
    fn missing_url_protocol_marker_is_detected_and_repaired_in_registry() {
        let fixture = TestProtocolFixture::new();
        register_codex_protocol_at(&fixture.location, &fixture.root, &fixture.handler, false)
            .expect("register protocol");
        let root_key = create_key(fixture.location.hive, &fixture.location.subkey)
            .expect("open protocol root");
        unsafe {
            delete_value(root_key.0, "URL Protocol").expect("remove URL Protocol marker");
        }
        notify_association_changed();

        assert_eq!(
            codex_protocol_status_at(&fixture.location, &fixture.root, &fixture.handler)
                .expect("inspect incomplete protocol"),
            CodexProtocolStatus::NeedsRepair
        );
        assert_eq!(
            register_codex_protocol_at(&fixture.location, &fixture.root, &fixture.handler, false)
                .expect("repair protocol"),
            ProtocolRegistration::Refreshed
        );
        assert_eq!(
            codex_protocol_status_at(&fixture.location, &fixture.root, &fixture.handler)
                .expect("inspect repaired protocol"),
            CodexProtocolStatus::Ready
        );
    }

    #[test]
    fn delegate_execute_requires_explicit_replacement() {
        let fixture = TestProtocolFixture::new();
        register_codex_protocol_at(&fixture.location, &fixture.root, &fixture.handler, false)
            .expect("register protocol");
        let command_key = create_key(fixture.location.hive, &fixture.location.command_subkey())
            .expect("open protocol command");
        unsafe {
            set_sz(command_key.0, "DelegateExecute", "")
                .expect("write conflicting delegate handler");
        }
        notify_association_changed();

        assert_eq!(
            codex_protocol_status_at(&fixture.location, &fixture.root, &fixture.handler)
                .expect("inspect delegated protocol"),
            CodexProtocolStatus::OtherOwner
        );
        assert_eq!(
            register_codex_protocol_at(&fixture.location, &fixture.root, &fixture.handler, false)
                .expect("preserve delegated protocol"),
            ProtocolRegistration::PreservedForeign
        );
        assert_eq!(
            read_sz_at(
                fixture.location.hive,
                &fixture.location.command_subkey(),
                Some("DelegateExecute"),
            )
            .expect("inspect preserved delegate handler")
            .as_deref(),
            Some("")
        );
        assert_eq!(
            register_codex_protocol_at(&fixture.location, &fixture.root, &fixture.handler, true)
                .expect("replace delegated protocol explicitly"),
            ProtocolRegistration::Refreshed
        );
        assert_eq!(
            read_sz_at(
                fixture.location.hive,
                &fixture.location.command_subkey(),
                Some("DelegateExecute"),
            )
            .expect("inspect repaired delegate handler"),
            None
        );
    }

    #[test]
    fn foreign_protocol_is_preserved_until_user_explicitly_replaces_it() {
        let fixture = TestProtocolFixture::new();
        let foreign_handler = fixture.write_effective_foreign_handler();
        let foreign_command = ProtocolValues::new(&foreign_handler).command;

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
            Some(foreign_command.as_str())
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
    fn uninstall_preserves_foreign_command_even_when_owned_markers_remain() {
        let fixture = TestProtocolFixture::new();
        register_codex_protocol_at(&fixture.location, &fixture.root, &fixture.handler, false)
            .expect("register protocol");
        let command_key = create_key(fixture.location.hive, &fixture.location.command_subkey())
            .expect("open protocol command");
        let foreign_command = r#""D:\Other Codex\ChatGPT.exe" "%1""#;
        unsafe {
            set_sz(command_key.0, "", foreign_command).expect("replace protocol command");
        }

        assert_eq!(
            remove_codex_protocol_at(&fixture.location, &fixture.root)
                .expect("preserve replaced protocol"),
            ProtocolRemoval::PreservedForeign
        );
        assert_eq!(
            read_sz_at(
                fixture.location.hive,
                &fixture.location.command_subkey(),
                None,
            )
            .expect("inspect preserved command")
            .as_deref(),
            Some(foreign_command)
        );
    }

    #[test]
    fn uninstall_preserves_delegate_only_handler_when_owned_markers_remain() {
        let fixture = TestProtocolFixture::new();
        register_codex_protocol_at(&fixture.location, &fixture.root, &fixture.handler, false)
            .expect("register protocol");
        let command_key = create_key(fixture.location.hive, &fixture.location.command_subkey())
            .expect("open protocol command");
        unsafe {
            delete_value(command_key.0, "").expect("remove protocol command");
            set_sz(command_key.0, "DelegateExecute", "{foreign-handler}")
                .expect("write delegate handler");
        }

        assert_eq!(
            remove_codex_protocol_at(&fixture.location, &fixture.root)
                .expect("preserve delegated protocol"),
            ProtocolRemoval::PreservedForeign
        );
        assert_eq!(
            read_sz_at(
                fixture.location.hive,
                &fixture.location.command_subkey(),
                Some("DelegateExecute"),
            )
            .expect("inspect preserved delegate")
            .as_deref(),
            Some("{foreign-handler}")
        );
    }

    #[test]
    fn missing_target_key_does_not_shadow_an_effective_foreign_handler() {
        let fixture = TestProtocolFixture::new();
        fixture.write_effective_foreign_handler();
        let shadow_location = fixture.shadow_location();

        assert_eq!(
            register_codex_protocol_at(&shadow_location, &fixture.root, &fixture.handler, false,)
                .expect("preserve effective foreign handler"),
            ProtocolRegistration::PreservedForeign
        );
        assert!(!key_exists(shadow_location.hive, &shadow_location.subkey)
            .expect("inspect missing shadow target"));
    }

    #[test]
    fn missing_target_is_created_even_when_effective_handler_is_owned() {
        let fixture = TestProtocolFixture::new();
        register_codex_protocol_at(&fixture.location, &fixture.root, &fixture.handler, false)
            .expect("register effective owned protocol");
        let shadow_location = fixture.shadow_location();

        assert_eq!(
            codex_protocol_status_at(&shadow_location, &fixture.root, &fixture.handler)
                .expect("inspect effective owned handler"),
            CodexProtocolStatus::Missing
        );
        assert_eq!(
            register_codex_protocol_at(&shadow_location, &fixture.root, &fixture.handler, false)
                .expect("create missing target registration"),
            ProtocolRegistration::Created
        );
        assert!(key_exists(shadow_location.hive, &shadow_location.subkey)
            .expect("inspect created target scope"));
    }

    #[test]
    fn machine_target_is_created_under_effective_foreign_handler() {
        let fixture = TestProtocolFixture::new();
        fixture.write_effective_foreign_handler();
        let machine_location = fixture.machine_shadow_location();

        assert_eq!(
            register_codex_protocol_at(&machine_location, &fixture.root, &fixture.handler, false)
                .expect("create machine target registration"),
            ProtocolRegistration::Created
        );
        assert_eq!(
            classify_protocol(
                &read_existing_protocol(&machine_location).expect("read machine target"),
                &fixture.root,
                &ProtocolValues::new(&fixture.handler),
            ),
            CodexProtocolStatus::Ready
        );
        assert_eq!(
            codex_protocol_status_at(&machine_location, &fixture.root, &fixture.handler)
                .expect("inspect shadowed machine target"),
            CodexProtocolStatus::OtherOwner
        );
    }

    #[test]
    fn ready_owned_target_is_kept_when_effective_handler_is_foreign() {
        let fixture = TestProtocolFixture::new();
        fixture.write_effective_foreign_handler();
        let shadow_location = fixture.shadow_location();
        write_protocol(
            &shadow_location,
            &fixture.root,
            &ProtocolValues::new(&fixture.handler),
        )
        .expect("write ready shadow target");

        assert_eq!(
            codex_protocol_status_at(&shadow_location, &fixture.root, &fixture.handler)
                .expect("inspect shadowed target"),
            CodexProtocolStatus::OtherOwner
        );
        assert_eq!(
            register_codex_protocol_at(&shadow_location, &fixture.root, &fixture.handler, false,)
                .expect("keep ready owned target"),
            ProtocolRegistration::Unchanged
        );
    }

    #[test]
    fn stale_owned_target_is_refreshed_under_effective_foreign_handler() {
        let fixture = TestProtocolFixture::new();
        fixture.write_effective_foreign_handler();
        let shadow_location = fixture.shadow_location();
        let old_handler = fixture
            .root
            .join("versions")
            .join("old")
            .join("ChatGPT.exe");
        std::fs::create_dir_all(old_handler.parent().expect("old handler parent"))
            .expect("create old handler directory");
        std::fs::write(&old_handler, b"old handler").expect("write old handler");
        write_protocol(
            &shadow_location,
            &fixture.root,
            &ProtocolValues::new(&old_handler),
        )
        .expect("write stale owned target");

        assert_eq!(
            register_codex_protocol_at(&shadow_location, &fixture.root, &fixture.handler, false,)
                .expect("refresh stale owned target"),
            ProtocolRegistration::Refreshed
        );
        assert_eq!(
            classify_protocol(
                &read_existing_protocol(&shadow_location).expect("read refreshed target"),
                &fixture.root,
                &ProtocolValues::new(&fixture.handler),
            ),
            CodexProtocolStatus::Ready
        );
        assert_eq!(
            codex_protocol_status_at(&shadow_location, &fixture.root, &fixture.handler)
                .expect("inspect shadowed refreshed target"),
            CodexProtocolStatus::OtherOwner
        );
    }

    #[test]
    fn ineffective_explicit_replacement_is_rolled_back() {
        let fixture = TestProtocolFixture::new();
        fixture.write_effective_foreign_handler();
        let shadow_location = fixture.shadow_location();

        assert!(register_codex_protocol_at(
            &shadow_location,
            &fixture.root,
            &fixture.handler,
            true,
        )
        .is_err());
        assert!(!key_exists(shadow_location.hive, &shadow_location.subkey)
            .expect("verify failed replacement rollback"));
    }

    #[test]
    fn failed_replacement_restores_existing_foreign_registration() {
        let fixture = TestProtocolFixture::new();
        fixture.write_effective_foreign_handler();
        let shadow_location = fixture.shadow_location();
        let previous_handler = fixture.root.join("previous").join("ChatGPT.exe");
        std::fs::create_dir_all(previous_handler.parent().expect("previous handler parent"))
            .expect("create previous handler directory");
        std::fs::write(&previous_handler, b"previous handler").expect("write previous handler");
        write_protocol(
            &shadow_location,
            &fixture.root.join("previous-install"),
            &ProtocolValues::new(&previous_handler),
        )
        .expect("write previous foreign target");
        let command_key = create_key(shadow_location.hive, &shadow_location.command_subkey())
            .expect("open previous command key");
        unsafe {
            set_sz(command_key.0, "DelegateExecute", "{previous-handler}")
                .expect("write previous delegate handler");
        }
        let before = read_protocol_snapshot(&shadow_location).expect("snapshot previous target");

        assert!(register_codex_protocol_at(
            &shadow_location,
            &fixture.root,
            &fixture.handler,
            true,
        )
        .is_err());
        assert_eq!(
            read_protocol_snapshot(&shadow_location).expect("inspect restored target"),
            before
        );
    }
}

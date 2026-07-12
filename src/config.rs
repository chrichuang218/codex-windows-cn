use crate::store::Fetcher;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Filename of our config, placed next to the launcher exe.
///
/// Named `updater.json` (not `config.json`) specifically because the install
/// root can coincide with Codex's own userData dir. Codex is Electron and
/// may write a file called `config.json` there; a collision would have us
/// overwriting its state. `updater.json` is a filename no Electron app
/// conventionally uses.
pub const CONFIG_FILENAME: &str = "updater.json";

/// Legacy filename used by pre-migration installs. Kept as a constant so
/// the migration path in `mode::detect` can find and rename it.
pub const LEGACY_CONFIG_FILENAME: &str = "config.json";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InstallMode {
    Portable,
    User,
    System,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum UpdatePolicy {
    Always,
    #[default]
    Daily,
    Weekly,
    Never,
}

/// Written next to `codex-launcher.exe` once installation completes.
/// Presence of this file is what makes the launcher run in proxy mode
/// instead of installer mode.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub install_mode: InstallMode,
    /// Version string (e.g. "40.0.0") — matches the `version` file shipped in the MSIX.
    pub current_version: String,
    #[serde(default)]
    pub update_policy: UpdatePolicy,
    #[serde(default)]
    pub last_check_unix: Option<u64>,
    #[serde(default)]
    pub last_launcher_check_unix: Option<u64>,
    /// If Some, skip the update prompt until this time. Used by the
    /// "don't ask again for 1 day / 7 days / ever" options.
    #[serde(default)]
    pub suppress_until_unix: Option<u64>,
    /// Last version we saw on the Store. May be newer than current_version
    /// if the user deferred an update.
    #[serde(default)]
    pub known_latest: Option<String>,
    /// Specific version the user chose "Skip this version" for. Suppresses
    /// prompts only as long as the Store's latest equals this; as soon as
    /// Microsoft publishes a newer version we prompt again.
    #[serde(default)]
    pub skipped_version: Option<String>,
    #[serde(default = "default_keep_versions")]
    pub keep_versions: u32,
    /// Disable automatic version pruning. Manual deletion remains available.
    #[serde(default)]
    pub keep_all_versions: bool,
    /// Which strategy the launcher uses to download MSIX updates. Currently
    /// not auto-flipped on fallback success — see `installer::update_inner`.
    #[serde(default)]
    pub fetcher: Fetcher,
    /// Maintain `versions/current` as a directory junction pointing at the
    /// newest installed version. Off by default is rare — the junction
    /// gives tooling/AV/shortcuts a stable path. Users can disable it at
    /// install time if their filesystem / AV doesn't play nicely with
    /// reparse points.
    #[serde(default = "default_true")]
    pub use_current_junction: bool,
    /// Whether the Add/Remove Programs registry entry exists. Update path
    /// uses this to know whether to refresh DisplayVersion / DisplayIcon.
    /// Off by default for Portable installs.
    #[serde(default = "default_true")]
    pub register_uninstall: bool,
    /// Whether this install should maintain the `codex://` URL protocol.
    /// `None` is a legacy config: User/System installs opt in, Portable
    /// installs stay registry-free until the user explicitly enables it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub register_codex_protocol: Option<bool>,
    /// Last GitHub release tag we observed for the launcher itself
    /// (this project, not Codex). Compared against `CARGO_PKG_VERSION`
    /// at runtime to detect when a newer launcher is available.
    #[serde(default)]
    pub known_latest_launcher: Option<String>,
    /// Specific launcher version the user chose "Skip this version" for.
    /// Suppresses the launcher prompt only as long as the GitHub release's
    /// latest tag equals this — newer releases re-prompt.
    #[serde(default)]
    pub skipped_launcher_version: Option<String>,
    /// Snooze for the launcher prompt only (independent of `suppress_until_unix`,
    /// which suppresses the *Codex* prompt). Set to `u64::MAX` for "Never".
    #[serde(default)]
    pub launcher_suppress_until_unix: Option<u64>,
}

fn default_keep_versions() -> u32 {
    5
}
fn default_true() -> bool {
    true
}

impl Config {
    pub fn register_codex_protocol_preference(&self) -> Option<bool> {
        self.register_codex_protocol
            .or_else(|| (!matches!(self.install_mode, InstallMode::Portable)).then_some(true))
    }

    pub fn register_codex_protocol_enabled(&self) -> bool {
        self.register_codex_protocol_preference().unwrap_or(false)
    }

    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&raw)?)
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let raw = serde_json::to_string_pretty(self)?;
        std::fs::write(path, raw)?;
        Ok(())
    }

    /// Load runtime config. Always reads the install-root `updater.json`
    /// first. For **System** installs, if a per-user state file exists
    /// with a matching `install_root`, returns that instead — the state
    /// file is the runtime-current view because the unelevated proxy
    /// can't write back to Program Files. For Portable/User installs
    /// the state file is never consulted (there's no permission gap to
    /// bridge).
    pub fn load_runtime(install_root: &Path) -> anyhow::Result<Self> {
        let cfg = Self::load(&install_root.join(CONFIG_FILENAME))?;
        if !matches!(cfg.install_mode, InstallMode::System) {
            return Ok(cfg);
        }
        if let Some(state_path) = state_file_path() {
            if state_path.exists() {
                if let Ok(raw) = std::fs::read_to_string(&state_path) {
                    if let Ok(state) = serde_json::from_str::<StateFile>(&raw) {
                        if paths_equal(&state.install_root, install_root) {
                            return Ok(state.config);
                        }
                    }
                }
            }
        }
        Ok(cfg)
    }

    /// Save runtime config to the appropriate location for the install mode:
    ///
    /// - **Portable / User**: writes directly to `<root>/updater.json`. The
    ///   install root is user-writable in both modes, so there's no fallback
    ///   path. A failure here means something is genuinely wrong (read-only
    ///   volume, AV, etc.) and is propagated.
    ///
    /// - **System**: writes to the per-user state file at
    ///   `%LOCALAPPDATA%\codex-launcher\state.json`. The install-root config
    ///   in `C:\Program Files\Codex` is fixed at install time (when the
    ///   wizard ran elevated) and the unelevated proxy can't update it.
    pub fn save_runtime(&self, install_root: &Path) -> anyhow::Result<()> {
        match self.install_mode {
            InstallMode::Portable | InstallMode::User => {
                self.save(&install_root.join(CONFIG_FILENAME))
            }
            InstallMode::System => {
                let state_path = state_file_path().ok_or_else(|| {
                    anyhow::anyhow!("LOCALAPPDATA not set; cannot persist runtime state")
                })?;
                if let Some(parent) = state_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let raw = serde_json::to_string_pretty(&StateFile {
                    install_root: install_root.to_path_buf(),
                    config: self.clone(),
                })?;
                std::fs::write(&state_path, raw)?;
                Ok(())
            }
        }
    }

    /// Save during install or update — contexts where the caller has
    /// elevation (or doesn't need it). Always writes to the install-root
    /// `updater.json` regardless of mode. For System installs, also
    /// clears any stale per-user state file that matches this install
    /// root: the freshly-written install-root config is now newer and
    /// shouldn't be shadowed by a left-over state overlay.
    pub fn save_install(&self, install_root: &Path) -> anyhow::Result<()> {
        self.save(&install_root.join(CONFIG_FILENAME))?;
        let _ = clear_state_file_if_ours(install_root);
        Ok(())
    }
}

/// Remove the per-user state file iff its embedded `install_root` matches.
/// Returns `Ok(Some(path))` if a matching state file was deleted, `Ok(None)`
/// if there was nothing to do (no LOCALAPPDATA, file missing, parse failure,
/// or different install), `Err` only on actual delete failure.
pub fn clear_state_file_if_ours(
    install_root: &Path,
) -> std::io::Result<Option<std::path::PathBuf>> {
    let Some(state_path) = state_file_path() else {
        return Ok(None);
    };
    let Ok(raw) = std::fs::read_to_string(&state_path) else {
        return Ok(None);
    };
    let Ok(state) = serde_json::from_str::<StateFile>(&raw) else {
        return Ok(None);
    };
    if !paths_equal(&state.install_root, install_root) {
        return Ok(None);
    }
    std::fs::remove_file(&state_path)?;
    Ok(Some(state_path))
}

/// Per-user fallback state file shape. `install_root` is embedded so we
/// can ignore stale state from a different install at the same machine.
///
/// TODO: currently the entire `Config` is serialized into the state file,
/// meaning install-time fields (install_mode, keep_versions, fetcher,
/// use_current_junction, register_uninstall) are also persisted and would
/// override their install-root values on load. In practice this is benign
/// — those fields don't change between writes — but if a future code path
/// fat-fingers one of them at runtime, the state file becomes the
/// authoritative answer. Tighten by splitting into a `RuntimeState`
/// struct holding only mutable fields (current_version, update_policy,
/// last_check_unix, last_launcher_check_unix, suppress_until_unix,
/// known_latest, skipped_version)
/// and overlaying onto the install-root config at load time. Not urgent.
#[derive(Debug, Serialize, Deserialize)]
struct StateFile {
    install_root: PathBuf,
    config: Config,
}

fn state_file_path() -> Option<PathBuf> {
    let base = std::env::var("LOCALAPPDATA").ok()?;
    Some(
        PathBuf::from(base)
            .join("codex-launcher")
            .join("state.json"),
    )
}

/// Best-effort path equality. Tries canonicalization first (resolves
/// short names, junctions, case differences); falls back to a normalized
/// lowercase string match if either side can't canonicalize.
fn paths_equal(a: &Path, b: &Path) -> bool {
    if let (Ok(ca), Ok(cb)) = (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        return ca == cb;
    }
    let norm = |p: &Path| {
        p.to_string_lossy()
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_ascii_lowercase()
    };
    norm(a) == norm(b)
}

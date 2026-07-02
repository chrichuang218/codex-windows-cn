//! Installer orchestration. Runs in a worker thread so the Slint event
//! loop stays responsive; posts [`InstallMsg`] updates back via a
//! user-provided callback.
//!
//! Steps executed:
//!   1. Download MSIX (Direct or Winget) into `<root>/downloads/`
//!   2. Extract `app/` subtree into `<root>/versions/<ver>/`
//!   3. Copy current launcher exe to `<root>/codex-launcher.exe` (so the
//!      stub lives next to updater.json and can proxy-launch later)
//!   4. Write `<root>/updater.json`
//!   5. Prune old versions to `keep_versions`
//!
//! Shortcuts / registry / UAC elevation are intentionally NOT here — they
//! live in their own modules alongside the rest of the Windows-integration goo.

use crate::config::{Config, InstallMode, UpdatePolicy};
use crate::extract;
use crate::junction;
use crate::registry;
use crate::shortcut;
use crate::store::{self, Fetcher};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct InstallOptions {
    pub mode: InstallMode,
    pub root: PathBuf,
    pub create_shortcut: bool,
    /// Write the Add/Remove Programs registry entry so the user can
    /// uninstall via Windows Settings → Apps. Off by default for Portable.
    pub register_uninstall: bool,
    pub keep_versions: u32,
    pub fetcher: Fetcher,
    /// Create `versions/current` junction pointing at the newest install.
    pub use_current_junction: bool,
    /// For LocalFile fetcher — user-supplied MSIX path. Ignored otherwise.
    pub local_msix: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub enum InstallMsg {
    /// Phase name (e.g. "Downloading", "Extracting") + detail line.
    Phase { phase: String, detail: String },
    /// Fraction in [0.0, 1.0]. Use `None` for indeterminate.
    Progress(Option<f32>),
    /// Installation finished successfully. Carries the installed version.
    Done { version: String },
    /// Installation failed. Carries a user-readable error message.
    Error(String),
}

/// Default install path for a given mode. Caller can override via UI.
pub fn default_path(mode: InstallMode) -> PathBuf {
    match mode {
        InstallMode::Portable => std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("CodexPortable"),
        InstallMode::User => directories::BaseDirs::new()
            .map(|d| d.data_local_dir().join("Codex"))
            .unwrap_or_else(|| PathBuf::from(r"C:\Users\Public\Codex")),
        InstallMode::System => PathBuf::from(r"C:\Program Files\Codex"),
    }
}

/// Synchronous install. Call from a worker thread. `on_msg` is invoked
/// with phase/progress updates AND the final Done/Error — callers should
/// treat a Done or Error as the last message they'll ever see.
pub fn run(opts: InstallOptions, on_msg: impl Fn(InstallMsg) + Send + 'static) {
    match run_inner(&opts, &on_msg) {
        Ok(version) => on_msg(InstallMsg::Done { version }),
        Err(e) => on_msg(InstallMsg::Error(format!("{:#}", e))),
    }
}

/// Update an existing install in-place. Loads `<root>/updater.json`, downloads
/// and extracts the latest MSIX, updates `current_version`, `known_latest`,
/// and `last_check_unix`, then prunes. Does NOT replace the launcher exe
/// (we're running from it). Does NOT change install_mode / keep_versions / etc.
pub fn update(root: std::path::PathBuf, on_msg: impl Fn(InstallMsg) + Send + 'static) {
    match update_inner(&root, &on_msg) {
        Ok(version) => on_msg(InstallMsg::Done { version }),
        Err(e) => on_msg(InstallMsg::Error(format!("{:#}", e))),
    }
}

fn update_inner(root: &Path, on_msg: &dyn Fn(InstallMsg)) -> Result<String> {
    // load_runtime, not load — the per-user state overlay (System installs)
    // holds runtime-current values like update_policy, skipped_version,
    // launcher_suppress_until_unix. save_install below clears the overlay,
    // so we must merge it in first or those choices vanish post-update.
    let mut cfg = Config::load_runtime(root)
        .with_context(|| format!("loading existing config at {}", root.display()))?;

    let downloads = root.join("downloads");
    std::fs::create_dir_all(&downloads)?;

    on_msg(InstallMsg::Phase {
        phase: "Downloading".into(),
        detail: format!("via {:?}", cfg.fetcher),
    });
    on_msg(InstallMsg::Progress(Some(0.0)));

    let boxed = BoxedFn(on_msg);
    // Discard `_succeeded_via`: a single transient failure (offline, proxy
    // blip) shouldn't permanently demote the user's chosen fetcher. A
    // stronger signal (N consecutive failures) would be needed first.
    let (result, _succeeded_via) = store::download_latest_with_fallback(
        cfg.fetcher,
        store::PRODUCT_ID_CODEX,
        &downloads,
        &mut |done, total| boxed.progress_bytes(done, total),
    )?;

    on_msg(InstallMsg::Phase {
        phase: "Extracting".into(),
        detail: format!("version {}", result.version),
    });
    on_msg(InstallMsg::Progress(Some(0.0)));

    let boxed = BoxedFn(on_msg);
    extract::extract_app(
        &result.msix_path,
        root,
        &result.version,
        &mut |done, total| {
            boxed.progress_entries(done, total);
        },
    )?;

    on_msg(InstallMsg::Phase {
        phase: "Finalizing".into(),
        detail: "updating config".into(),
    });
    on_msg(InstallMsg::Progress(None));

    cfg.current_version = result.version.clone();
    cfg.known_latest = Some(result.version.clone());
    cfg.suppress_until_unix = None;
    cfg.last_check_unix = Some(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    );
    // Update flow is elevated for System (UAC re-spawn) and runs in user
    // context for User/Portable, so install-root write is always available
    // here. save_install also clears any stale per-user state overlay so
    // the freshly-written install-root config takes effect on next launch.
    cfg.save_install(root)?;

    // Refresh junction to point at the new version. If the user disabled it
    // mid-life, tear down any stale link left from a previous install.
    let junction_link = root.join("versions").join("current");
    if cfg.use_current_junction {
        if let Err(e) = junction::set_current(root, &result.version) {
            eprintln!("warn: couldn't set versions/current junction: {e:#}");
        }
    } else {
        let _ = junction::remove(&junction_link);
    }

    // Refresh Start Menu shortcut icon so it picks up the new version's
    // Codex.exe, and bump the registry DisplayVersion/DisplayIcon so
    // Add/Remove Programs stays accurate.
    if let Ok(Some(link)) = shortcut::link_path(cfg.install_mode) {
        if link.exists() {
            if let Err(e) = write_shortcut(root, cfg.install_mode, &result.version) {
                eprintln!("warn: shortcut refresh: {e:#}");
            }
        }
    }
    if cfg.register_uninstall {
        if let Err(e) = write_registry(root, cfg.install_mode, &result.version) {
            eprintln!("warn: registry refresh: {e:#}");
        }
    }

    let _ = extract::prune_versions(root, cfg.keep_versions);
    let _ = std::fs::remove_file(&result.msix_path);

    Ok(result.version)
}

fn run_inner(opts: &InstallOptions, on_msg: &dyn Fn(InstallMsg)) -> Result<String> {
    std::fs::create_dir_all(&opts.root)
        .with_context(|| format!("creating install root {}", opts.root.display()))?;
    let downloads = opts.root.join("downloads");
    std::fs::create_dir_all(&downloads)?;

    // --- 1. Download --------------------------------------------------------
    on_msg(InstallMsg::Phase {
        phase: "Downloading".into(),
        detail: format!("via {:?}", opts.fetcher),
    });
    on_msg(InstallMsg::Progress(Some(0.0)));

    let result = match opts.fetcher {
        Fetcher::LocalFile => {
            let src = opts
                .local_msix
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("LocalFile fetcher needs a file path"))?;
            let on_msg_clone = BoxedFn(on_msg);
            store::local_file::from_file(src, &downloads, &mut |done, total| {
                on_msg_clone.progress_bytes(done, total);
            })?
        }
        _ => {
            let on_msg_clone = BoxedFn(on_msg);
            // See the note in `update_inner` for why we discard `_succeeded_via`.
            let (r, _succeeded_via) = store::download_latest_with_fallback(
                opts.fetcher,
                store::PRODUCT_ID_CODEX,
                &downloads,
                &mut |done, total| on_msg_clone.progress_bytes(done, total),
            )?;
            r
        }
    };

    // --- 2. Extract ---------------------------------------------------------
    on_msg(InstallMsg::Phase {
        phase: "Extracting".into(),
        detail: format!("version {}", result.version),
    });
    on_msg(InstallMsg::Progress(Some(0.0)));

    let on_msg_clone = BoxedFn(on_msg);
    extract::extract_app(
        &result.msix_path,
        &opts.root,
        &result.version,
        &mut |done, total| {
            on_msg_clone.progress_entries(done, total);
        },
    )?;

    // --- 3. Place launcher stub --------------------------------------------
    on_msg(InstallMsg::Phase {
        phase: "Finalizing".into(),
        detail: "installing launcher".into(),
    });
    on_msg(InstallMsg::Progress(None));

    place_launcher(&opts.root)?;

    // --- 4. Write config ----------------------------------------------------
    let cfg = Config {
        install_mode: opts.mode,
        current_version: result.version.clone(),
        update_policy: UpdatePolicy::default(),
        last_check_unix: None,
        suppress_until_unix: None,
        known_latest: Some(result.version.clone()),
        skipped_version: None,
        keep_versions: opts.keep_versions.max(1),
        fetcher: match opts.fetcher {
            Fetcher::LocalFile => Fetcher::Direct, // don't persist LocalFile
            f => f,
        },
        use_current_junction: opts.use_current_junction,
        register_uninstall: opts.register_uninstall,
        known_latest_launcher: None,
        skipped_launcher_version: None,
        launcher_suppress_until_unix: None,
    };
    // Initial install runs elevated for System mode, so install-root
    // write always succeeds. save_install also clears any stale state
    // overlay from a previous install at this same root.
    cfg.save_install(&opts.root)?;

    // --- 5. versions/current junction --------------------------------------
    if cfg.use_current_junction {
        if let Err(e) = junction::set_current(&opts.root, &result.version) {
            eprintln!("warn: couldn't set versions/current junction: {e:#}");
        }
    }

    // --- 6. Shortcut + registry -------------------------------------------
    if opts.create_shortcut {
        if let Err(e) = write_shortcut(&opts.root, opts.mode, &result.version) {
            eprintln!("warn: shortcut: {e:#}");
        }
    }
    if opts.register_uninstall {
        if let Err(e) = write_registry(&opts.root, opts.mode, &result.version) {
            eprintln!("warn: registry uninstall entry: {e:#}");
        }
    }

    // --- 7. Prune -----------------------------------------------------------
    let _ = extract::prune_versions(&opts.root, cfg.keep_versions);

    // --- 8. Clean downloaded MSIX ------------------------------------------
    // The extracted version is already on disk — no need to hoard the
    // ~400MB package. (We keep `downloads/` itself for future updates.)
    let _ = std::fs::remove_file(&result.msix_path);

    Ok(result.version)
}

/// Copy the currently-running launcher to `<root>/codex-launcher.exe`.
/// Must be a no-op if source == dest (e.g. someone re-runs the installer
/// from inside the install directory to reconfigure).
fn place_launcher(root: &Path) -> Result<()> {
    let src = std::env::current_exe().context("current_exe()")?;
    let dest = root.join("codex-launcher.exe");
    if same_file(&src, &dest) {
        // We're already running from the install location — nothing to do.
        return Ok(());
    }
    std::fs::copy(&src, &dest)
        .with_context(|| format!("copying launcher {} → {}", src.display(), dest.display()))?;
    Ok(())
}

/// Best-effort same-file check that tolerates differing path spellings
/// (UNC vs drive-letter, `..`, short names). Falls back to direct path
/// equality if either side can't be canonicalized (e.g. dest doesn't exist).
fn same_file(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => false,
    }
}

/// (Re)create the Start Menu `.lnk` with icon pointing at this version's
/// `Codex.exe`. No-op for Portable mode (link_path returns None).
fn write_shortcut(root: &Path, mode: InstallMode, version: &str) -> Result<()> {
    let Some(link) = shortcut::link_path(mode)? else {
        return Ok(());
    };
    let target = root.join("codex-launcher.exe");
    let icon = root.join("versions").join(version).join("Codex.exe");
    shortcut::create_or_update(&link, &target, &icon, "Codex (unofficial updater)", root)
}

/// (Re)write the Add/Remove Programs registry entry for the current install.
fn write_registry(root: &Path, mode: InstallMode, version: &str) -> Result<()> {
    let launcher = root.join("codex-launcher.exe");
    let icon = root.join("versions").join(version).join("Codex.exe");
    let entry = registry::UninstallEntry {
        display_name: "Codex (unofficial updater)",
        display_version: version,
        publisher: "vaportail",
        install_location: root,
        uninstall_string: format!("\"{}\" --uninstall", launcher.display()),
        display_icon: &icon,
    };
    registry::write(mode, &entry)
}

/// Helper to pass a `&dyn Fn(InstallMsg)` into a `FnMut(u64, Option<u64>)`
/// progress callback without cloning the closure.
struct BoxedFn<'a>(&'a dyn Fn(InstallMsg));

impl BoxedFn<'_> {
    fn progress_bytes(&self, done: u64, total: Option<u64>) {
        let frac = total
            .filter(|t| *t > 0)
            .map(|t| (done as f32 / t as f32).clamp(0.0, 1.0));
        let detail = match total {
            Some(t) => format!("{} / {} MB", done / 1_048_576, t / 1_048_576),
            None => format!("{} MB", done / 1_048_576),
        };
        (self.0)(InstallMsg::Phase {
            phase: "Downloading".into(),
            detail,
        });
        (self.0)(InstallMsg::Progress(frac));
    }

    fn progress_entries(&self, done: u64, total: Option<u64>) {
        let frac = total
            .filter(|t| *t > 0)
            .map(|t| (done as f32 / t as f32).clamp(0.0, 1.0));
        let detail = match total {
            Some(t) => format!("{done} / {t} files"),
            None => format!("{done} files"),
        };
        (self.0)(InstallMsg::Phase {
            phase: "Extracting".into(),
            detail,
        });
        (self.0)(InstallMsg::Progress(frac));
    }
}

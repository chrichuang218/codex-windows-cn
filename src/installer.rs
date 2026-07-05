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
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

const MIN_UPDATE_EXTRACT_VISIBLE: Duration = Duration::from_millis(900);

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
    /// Progress with its own phase/detail so polling the last event is enough
    /// to render a useful UI.
    ProgressDetail {
        phase: String,
        detail: String,
        progress: Option<f32>,
    },
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
    match run_inner(&opts, &on_msg, None) {
        Ok(version) => on_msg(InstallMsg::Done { version }),
        Err(e) => on_msg(InstallMsg::Error(format!("{:#}", e))),
    }
}

pub fn run_cancellable(
    opts: InstallOptions,
    cancel: Arc<AtomicBool>,
    on_msg: impl Fn(InstallMsg) + Send + 'static,
) {
    match run_inner(&opts, &on_msg, Some(cancel.as_ref())) {
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

    let update_fetcher = match cfg.fetcher {
        Fetcher::Direct | Fetcher::LocalFile => Fetcher::Winget,
        Fetcher::Winget => Fetcher::Winget,
    };

    emit_progress_detail(
        on_msg,
        "Downloading",
        &update_start_detail(update_fetcher),
        None,
    );

    let boxed = BoxedFn(on_msg, None);
    // Discard `_succeeded_via`: a single transient failure (offline, proxy
    // blip) shouldn't permanently demote the user's chosen fetcher. A
    // stronger signal (N consecutive failures) would be needed first.
    let (result, _succeeded_via) = store::download_latest_with_fallback(
        update_fetcher,
        store::PRODUCT_ID_CODEX,
        &downloads,
        &mut |done, total| boxed.progress_bytes(done, total),
    )?;

    emit_progress_detail(
        on_msg,
        "Extracting",
        &format!("正在准备解压 Codex {}。", result.version),
        None,
    );

    let extract_started = Instant::now();
    let boxed = BoxedFn(on_msg, None);
    extract::extract_app(
        &result.msix_path,
        root,
        &result.version,
        &mut |done, total| boxed.progress_entries(done, total),
    )?;
    let elapsed = extract_started.elapsed();
    if elapsed < MIN_UPDATE_EXTRACT_VISIBLE {
        std::thread::sleep(MIN_UPDATE_EXTRACT_VISIBLE - elapsed);
    }

    emit_progress_detail(on_msg, "Finalizing", "正在写入更新配置。", None);

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

fn run_inner(
    opts: &InstallOptions,
    on_msg: &dyn Fn(InstallMsg),
    cancel: Option<&AtomicBool>,
) -> Result<String> {
    check_cancel(cancel)?;
    std::fs::create_dir_all(&opts.root)
        .with_context(|| format!("creating install root {}", opts.root.display()))?;
    let downloads = opts.root.join("downloads");
    std::fs::create_dir_all(&downloads)?;

    // --- 1. Download --------------------------------------------------------
    emit_progress_detail(
        on_msg,
        "Downloading",
        &download_start_detail(opts.fetcher),
        None,
    );

    let result = match opts.fetcher {
        Fetcher::LocalFile => {
            check_cancel(cancel)?;
            let src = opts
                .local_msix
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("LocalFile fetcher needs a file path"))?;
            let on_msg_clone = BoxedFn(on_msg, cancel);
            store::local_file::from_file(src, &downloads, &mut |done, total| {
                on_msg_clone.progress_bytes(done, total)
            })?
        }
        _ => {
            check_cancel(cancel)?;
            let on_msg_clone = BoxedFn(on_msg, cancel);
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
    emit_progress_detail(
        on_msg,
        "Extracting",
        &format!("正在准备解压 Codex {}。", result.version),
        None,
    );

    check_cancel(cancel)?;
    let on_msg_clone = BoxedFn(on_msg, cancel);
    extract::extract_app(
        &result.msix_path,
        &opts.root,
        &result.version,
        &mut |done, total| on_msg_clone.progress_entries(done, total),
    )?;

    // --- 3. Place launcher stub --------------------------------------------
    emit_progress_detail(on_msg, "Finalizing", "正在写入启动器和配置。", None);

    check_cancel(cancel)?;
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
    check_cancel(cancel)?;
    cfg.save_install(&opts.root)?;

    // --- 5. Best-effort Windows integration + cleanup -----------------------
    //
    // At this point the install is usable: Codex is extracted, the launcher is
    // in place, and updater.json is written. Junctions, shortcuts, registry
    // metadata, pruning, and MSIX cleanup are useful polish, but should not be
    // allowed to keep the UI stuck on the final progress screen if Windows
    // shell integration or antivirus is slow.
    let post_install = PostInstallWork {
        root: opts.root.clone(),
        version: result.version.clone(),
        mode: opts.mode,
        create_shortcut: opts.create_shortcut,
        register_uninstall: opts.register_uninstall,
        keep_versions: cfg.keep_versions,
        use_current_junction: cfg.use_current_junction,
        msix_path: result.msix_path.clone(),
    };
    std::thread::spawn(move || run_post_install_work(post_install));

    Ok(result.version)
}

struct PostInstallWork {
    root: PathBuf,
    version: String,
    mode: InstallMode,
    create_shortcut: bool,
    register_uninstall: bool,
    keep_versions: u32,
    use_current_junction: bool,
    msix_path: PathBuf,
}

fn run_post_install_work(work: PostInstallWork) {
    if work.use_current_junction {
        if let Err(e) = junction::set_current(&work.root, &work.version) {
            eprintln!("warn: couldn't set versions/current junction: {e:#}");
        }
    }
    if work.create_shortcut {
        if let Err(e) = write_shortcut(&work.root, work.mode, &work.version) {
            eprintln!("warn: shortcut: {e:#}");
        }
    }
    if work.register_uninstall {
        if let Err(e) = write_registry(&work.root, work.mode, &work.version) {
            eprintln!("warn: registry uninstall entry: {e:#}");
        }
    }
    let _ = extract::prune_versions(&work.root, work.keep_versions);
    let _ = std::fs::remove_file(&work.msix_path);
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

/// (Re)create the Start Menu `.lnk` with the launcher's embedded icon.
/// No-op for Portable mode (link_path returns None).
fn write_shortcut(root: &Path, mode: InstallMode, _version: &str) -> Result<()> {
    let Some(link) = shortcut::link_path(mode)? else {
        return Ok(());
    };
    let target = root.join("codex-launcher.exe");
    let icon = target.clone();
    shortcut::create_or_update(&link, &target, &icon, "Codex (unofficial updater)", root)
}

/// (Re)write the Add/Remove Programs registry entry for the current install.
fn write_registry(root: &Path, mode: InstallMode, version: &str) -> Result<()> {
    let launcher = root.join("codex-launcher.exe");
    let entry = registry::UninstallEntry {
        display_name: "Codex (unofficial updater)",
        display_version: version,
        publisher: "vaportail",
        install_location: root,
        uninstall_string: format!("\"{}\" --uninstall", launcher.display()),
        display_icon: &launcher,
    };
    registry::write(mode, &entry)
}

fn emit_progress_detail(
    on_msg: &dyn Fn(InstallMsg),
    phase: &str,
    detail: &str,
    progress: Option<f32>,
) {
    on_msg(InstallMsg::ProgressDetail {
        phase: phase.into(),
        detail: detail.into(),
        progress,
    });
}

fn download_start_detail(fetcher: Fetcher) -> String {
    match fetcher {
        Fetcher::Direct => "正在解析 Codex 下载地址；如果直连超时，将自动切换 winget。".into(),
        Fetcher::Winget => "正在调用 winget 下载 Codex。".into(),
        Fetcher::LocalFile => "正在复制本地 MSIX 文件。".into(),
    }
}

fn update_start_detail(fetcher: Fetcher) -> String {
    match fetcher {
        Fetcher::Winget => "正在调用 winget 下载 Codex；如果 winget 失败，将自动切换直连。".into(),
        other => download_start_detail(other),
    }
}

/// Helper to pass a `&dyn Fn(InstallMsg)` into a `FnMut(u64, Option<u64>)`
/// progress callback without cloning the closure.
struct BoxedFn<'a>(&'a dyn Fn(InstallMsg), Option<&'a AtomicBool>);

impl BoxedFn<'_> {
    fn progress_bytes(&self, done: u64, total: Option<u64>) -> Result<()> {
        check_cancel(self.1)?;
        let frac = total
            .filter(|t| *t > 0)
            .map(|t| (done as f32 / t as f32).clamp(0.0, 1.0));
        let detail = match total {
            Some(t) => format!("{} / {} MB", done / 1_048_576, t / 1_048_576),
            None => format!("{} MB", done / 1_048_576),
        };
        (self.0)(InstallMsg::ProgressDetail {
            phase: "Downloading".into(),
            detail,
            progress: frac,
        });
        Ok(())
    }

    fn progress_entries(&self, done: u64, total: Option<u64>) -> Result<()> {
        check_cancel(self.1)?;
        let frac = total
            .filter(|t| *t > 0)
            .map(|t| (done as f32 / t as f32).clamp(0.0, 1.0));
        let detail = match total {
            Some(t) => format!("{done} / {t} files"),
            None => format!("{done} files"),
        };
        (self.0)(InstallMsg::ProgressDetail {
            phase: "Extracting".into(),
            detail,
            progress: frac,
        });
        Ok(())
    }
}

fn check_cancel(cancel: Option<&AtomicBool>) -> Result<()> {
    if cancel
        .map(|flag| flag.load(Ordering::SeqCst))
        .unwrap_or(false)
    {
        anyhow::bail!("安装已取消");
    }
    Ok(())
}

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
use crate::dialogs;
use crate::extract;
use crate::junction;
use crate::registry;
use crate::shortcut;
use crate::store::{self, DownloadResult, Fetcher};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

const MIN_UPDATE_EXTRACT_VISIBLE: Duration = Duration::from_millis(900);
const DIRECT_DOWNLOAD_ATTEMPTS: u32 = 3;

#[derive(Debug, Clone)]
pub struct InstallOptions {
    pub mode: InstallMode,
    pub root: PathBuf,
    pub create_shortcut: bool,
    pub create_desktop_shortcut: bool,
    pub create_assistant_desktop_shortcut: bool,
    /// Write the Add/Remove Programs registry entry so the user can
    /// uninstall via Windows Settings → Apps. Off by default for Portable.
    pub register_uninstall: bool,
    pub keep_versions: u32,
    pub keep_all_versions: bool,
    pub fetcher: Fetcher,
    /// Create `versions/current` junction pointing at the newest install.
    pub use_current_junction: bool,
    /// For LocalFile fetcher — user-supplied MSIX path. Ignored otherwise.
    pub local_msix: Option<PathBuf>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
    let report = |msg| {
        on_msg(msg);
        Ok(())
    };
    match run_inner(&opts, &report, None, PostWorkMode::Background) {
        Ok(version) => on_msg(InstallMsg::Done { version }),
        Err(e) => on_msg(InstallMsg::Error(format!("{:#}", e))),
    }
}

pub fn run_cancellable(
    opts: InstallOptions,
    cancel: Arc<AtomicBool>,
    on_msg: impl Fn(InstallMsg) + Send + 'static,
) {
    let report = |msg| {
        on_msg(msg);
        Ok(())
    };
    match run_inner(
        &opts,
        &report,
        Some(cancel.as_ref()),
        PostWorkMode::Background,
    ) {
        Ok(version) => on_msg(InstallMsg::Done { version }),
        Err(e) => on_msg(InstallMsg::Error(format!("{:#}", e))),
    }
}

/// Run a System install inside the one-shot elevated helper. Post-install
/// Windows integration must finish before the helper process exits.
pub fn run_elevated(
    opts: InstallOptions,
    on_msg: impl Fn(InstallMsg) -> Result<()>,
) -> Result<String> {
    run_inner(&opts, &on_msg, None, PostWorkMode::Inline)
}

/// Update an existing install in-place. Loads `<root>/updater.json`, downloads
/// and extracts the latest MSIX, updates `current_version`, `known_latest`,
/// and `last_check_unix`, then prunes. Does NOT replace the launcher exe
/// (we're running from it). Does NOT change install_mode / keep_versions / etc.
pub fn update(root: std::path::PathBuf, on_msg: impl Fn(InstallMsg) + Send + 'static) {
    let report = |msg| {
        on_msg(msg);
        Ok(())
    };
    match update_inner(&root, &report, PostWorkMode::Background) {
        Ok(version) => on_msg(InstallMsg::Done { version }),
        Err(e) => on_msg(InstallMsg::Error(format!("{:#}", e))),
    }
}

/// Run a System update inside the one-shot elevated helper. Post-update
/// Windows integration must finish before the helper process exits.
pub fn update_elevated(root: PathBuf, on_msg: impl Fn(InstallMsg) -> Result<()>) -> Result<String> {
    update_inner(&root, &on_msg, PostWorkMode::Inline)
}

#[derive(Clone, Copy)]
enum PostWorkMode {
    Background,
    Inline,
}

fn update_inner(
    root: &Path,
    on_msg: &dyn Fn(InstallMsg) -> Result<()>,
    post_work_mode: PostWorkMode,
) -> Result<String> {
    // load_runtime, not load — the per-user state overlay (System installs)
    // holds runtime-current values like update_policy, skipped_version,
    // launcher_suppress_until_unix. save_install below clears the overlay,
    // so we must merge it in first or those choices vanish post-update.
    let mut cfg = Config::load_runtime(root)
        .with_context(|| format!("loading existing config at {}", root.display()))?;

    let downloads = root.join("downloads");
    std::fs::create_dir_all(&downloads)?;

    let update_fetcher = configured_update_fetcher(cfg.fetcher);
    emit_progress_detail(
        on_msg,
        "Downloading",
        &download_start_detail(update_fetcher),
        None,
    )?;

    let result = download_with_retry(update_fetcher, on_msg, None, |progress| {
        store::download_latest(
            update_fetcher,
            store::PRODUCT_ID_CODEX,
            &downloads,
            progress,
        )
    })?;

    emit_progress_detail(
        on_msg,
        "Extracting",
        &format!("正在准备解压 Codex {}。", result.version),
        None,
    )?;

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

    emit_progress_detail(on_msg, "Finalizing", "正在写入更新配置。", None)?;

    // Reload just before committing so settings changed while the large
    // download/extract was running are preserved instead of being overwritten
    // by the snapshot loaded at update start.
    cfg = Config::load_runtime(root)
        .with_context(|| format!("reloading runtime config at {}", root.display()))?;
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

    // At this point the update is usable: the new version is extracted and
    // updater.json points at it. Keep Windows shell integration and cleanup
    // out of the critical path so slow AV / registry / shortcut work cannot
    // leave the UI stuck on the final progress screen.
    let post_update = PostUpdateWork {
        root: root.to_path_buf(),
        version: result.version.clone(),
        mode: cfg.install_mode,
        register_uninstall: cfg.register_uninstall,
        keep_versions: cfg.keep_versions,
        keep_all_versions: cfg.keep_all_versions,
        use_current_junction: cfg.use_current_junction,
        msix_path: result.msix_path.clone(),
    };
    match post_work_mode {
        PostWorkMode::Background => {
            std::thread::spawn(move || run_post_update_work(post_update));
        }
        PostWorkMode::Inline => run_post_update_work(post_update),
    }

    Ok(result.version)
}

fn run_inner(
    opts: &InstallOptions,
    on_msg: &dyn Fn(InstallMsg) -> Result<()>,
    cancel: Option<&AtomicBool>,
    post_work_mode: PostWorkMode,
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
    )?;

    let result = match opts.fetcher {
        Fetcher::LocalFile => {
            check_cancel(cancel)?;
            let src = opts
                .local_msix
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("LocalFile fetcher needs a file path"))?;
            let on_msg_clone = BoxedFn(on_msg, cancel);
            store::local_file::from_file(src, &downloads, &mut |done, total| {
                on_msg_clone.progress_bytes(done, total, None)
            })?
        }
        _ => {
            check_cancel(cancel)?;
            download_with_retry(opts.fetcher, on_msg, cancel, |progress| {
                store::download_latest(opts.fetcher, store::PRODUCT_ID_CODEX, &downloads, progress)
            })?
        }
    };

    // --- 2. Extract ---------------------------------------------------------
    emit_progress_detail(
        on_msg,
        "Extracting",
        &format!("正在准备解压 Codex {}。", result.version),
        None,
    )?;

    check_cancel(cancel)?;
    let on_msg_clone = BoxedFn(on_msg, cancel);
    extract::extract_app(
        &result.msix_path,
        &opts.root,
        &result.version,
        &mut |done, total| on_msg_clone.progress_entries(done, total),
    )?;

    // --- 3. Place launcher stub --------------------------------------------
    emit_progress_detail(on_msg, "Finalizing", "正在写入启动器和配置。", None)?;

    check_cancel(cancel)?;
    place_launcher(&opts.root)?;

    // --- 4. Write config ----------------------------------------------------
    let cfg = Config {
        install_mode: opts.mode,
        current_version: result.version.clone(),
        update_policy: UpdatePolicy::Always,
        last_check_unix: None,
        last_launcher_check_unix: None,
        suppress_until_unix: None,
        known_latest: Some(result.version.clone()),
        skipped_version: None,
        keep_versions: opts.keep_versions.max(1),
        keep_all_versions: opts.keep_all_versions,
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
        create_desktop_shortcut: opts.create_desktop_shortcut,
        create_assistant_desktop_shortcut: opts.create_assistant_desktop_shortcut,
        register_uninstall: opts.register_uninstall,
        keep_versions: cfg.keep_versions,
        keep_all_versions: cfg.keep_all_versions,
        use_current_junction: cfg.use_current_junction,
        msix_path: result.msix_path.clone(),
    };
    match post_work_mode {
        PostWorkMode::Background => {
            std::thread::spawn(move || run_post_install_work(post_install));
        }
        PostWorkMode::Inline => run_post_install_work(post_install),
    }

    Ok(result.version)
}

struct PostInstallWork {
    root: PathBuf,
    version: String,
    mode: InstallMode,
    create_shortcut: bool,
    create_desktop_shortcut: bool,
    create_assistant_desktop_shortcut: bool,
    register_uninstall: bool,
    keep_versions: u32,
    keep_all_versions: bool,
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
        if let Err(e) = write_start_menu_shortcut(&work.root, work.mode) {
            eprintln!("warn: shortcut: {e:#}");
            dialogs::error(&format!(
                "创建中文助手开始菜单快捷方式失败：{e:#}\n\n可重新运行安装程序后重试。"
            ));
        }
    }
    if work.create_desktop_shortcut {
        if let Err(e) =
            create_or_update_desktop_shortcut(&work.root, work.mode, work.use_current_junction)
        {
            eprintln!("warn: desktop shortcut: {e:#}");
            dialogs::error(&format!(
                "创建 ChatGPT 桌面快捷方式失败：{e:#}\n\n可稍后在中文助手的设置页重试。"
            ));
        }
    }
    if work.create_assistant_desktop_shortcut {
        if let Err(e) = create_or_update_assistant_desktop_shortcut(&work.root, work.mode) {
            eprintln!("warn: assistant desktop shortcut: {e:#}");
            dialogs::error(&format!(
                "创建中文助手桌面快捷方式失败：{e:#}\n\n可稍后在中文助手的设置页重试。"
            ));
        }
    }
    if work.register_uninstall {
        if let Err(e) = write_registry(&work.root, work.mode, &work.version) {
            eprintln!("warn: registry uninstall entry: {e:#}");
        }
    }
    prune_versions(&work.root, work.keep_versions, work.keep_all_versions);
    let _ = std::fs::remove_file(&work.msix_path);
}

struct PostUpdateWork {
    root: PathBuf,
    version: String,
    mode: InstallMode,
    register_uninstall: bool,
    keep_versions: u32,
    keep_all_versions: bool,
    use_current_junction: bool,
    msix_path: PathBuf,
}

fn run_post_update_work(work: PostUpdateWork) {
    let junction_link = work.root.join("versions").join("current");
    if work.use_current_junction {
        if let Err(e) = junction::set_current(&work.root, &work.version) {
            eprintln!("warn: couldn't set versions/current junction: {e:#}");
        }
    } else {
        let _ = junction::remove(&junction_link);
    }

    match start_menu_shortcut_exists(&work.root, work.mode) {
        Ok(true) => {
            if let Err(e) = write_start_menu_shortcut(&work.root, work.mode) {
                eprintln!("warn: shortcut refresh: {e:#}");
                dialogs::error(&format!(
                    "更新中文助手开始菜单快捷方式失败：{e:#}\n\n可重新运行安装程序后重试。"
                ));
            }
        }
        Ok(false) => {}
        Err(e) => {
            eprintln!("warn: start menu shortcut inspection: {e:#}");
            dialogs::error(&format!(
                "检查中文助手开始菜单快捷方式失败：{e:#}\n\n可重新运行安装程序后重试。"
            ));
        }
    }
    match desktop_shortcut_exists(&work.root, work.mode) {
        Ok(true) => {
            if let Err(e) =
                create_or_update_desktop_shortcut(&work.root, work.mode, work.use_current_junction)
            {
                eprintln!("warn: desktop shortcut refresh: {e:#}");
                dialogs::error(&format!(
                    "更新 ChatGPT 桌面快捷方式失败：{e:#}\n\n可在中文助手的设置页重新创建。"
                ));
            }
        }
        Ok(false) => {}
        Err(e) => {
            eprintln!("warn: desktop shortcut inspection: {e:#}");
            dialogs::error(&format!(
                "检查 ChatGPT 桌面快捷方式失败：{e:#}\n\n可在中文助手的设置页重新创建。"
            ));
        }
    }
    match assistant_desktop_shortcut_exists(&work.root, work.mode) {
        Ok(true) => {
            if let Err(e) = create_or_update_assistant_desktop_shortcut(&work.root, work.mode) {
                eprintln!("warn: assistant desktop shortcut refresh: {e:#}");
                dialogs::error(&format!(
                    "更新中文助手桌面快捷方式失败：{e:#}\n\n可在中文助手的设置页重新创建。"
                ));
            }
        }
        Ok(false) => {}
        Err(e) => {
            eprintln!("warn: assistant desktop shortcut inspection: {e:#}");
            dialogs::error(&format!(
                "检查中文助手桌面快捷方式失败：{e:#}\n\n可在中文助手的设置页重新创建。"
            ));
        }
    }
    if work.register_uninstall {
        if let Err(e) = write_registry(&work.root, work.mode, &work.version) {
            eprintln!("warn: registry refresh: {e:#}");
        }
    }

    prune_versions(&work.root, work.keep_versions, work.keep_all_versions);
    let _ = std::fs::remove_file(&work.msix_path);
}

fn prune_versions(root: &Path, keep_versions: u32, keep_all_versions: bool) {
    let policy = if keep_all_versions {
        crate::versions::RetentionPolicy::KeepAll
    } else {
        crate::versions::RetentionPolicy::KeepLatest(keep_versions)
    };
    let running = crate::proxy::running_versions(&root.join("versions"));
    if let Err(error) = crate::versions::prune_installed(root, policy, &running) {
        eprintln!("warn: version pruning failed: {error:#}");
    }
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
fn write_start_menu_shortcut(root: &Path, mode: InstallMode) -> Result<()> {
    let Some(link) = shortcut::link_path(mode)? else {
        return Ok(());
    };
    let legacy_link = shortcut::legacy_link_path(mode)?;
    let target = root.join("codex-launcher.exe");
    if link.exists() && !shortcut::is_owned(&link, &target, "")? {
        anyhow::bail!(
            "开始菜单已存在其他来源的 Codex Windows 中文助手快捷方式，为避免覆盖已保留原文件"
        );
    }
    let remove_legacy = match legacy_link.as_deref() {
        Some(path) => shortcut::is_owned(path, &target, "")?,
        None => false,
    };
    let icon = target.clone();
    shortcut::create_or_update(&link, &target, &icon, "Codex Windows 中文助手", root, "")?;
    if remove_legacy {
        if let Some(legacy_link) = legacy_link {
            shortcut::remove(&legacy_link)?;
        }
    }
    Ok(())
}

pub fn start_menu_shortcut_exists(root: &Path, mode: InstallMode) -> Result<bool> {
    let target = root.join("codex-launcher.exe");
    for link in [
        shortcut::link_path(mode)?,
        shortcut::legacy_link_path(mode)?,
    ]
    .into_iter()
    .flatten()
    {
        if shortcut::is_owned(&link, &target, "")? {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn remove_start_menu_shortcut(root: &Path, mode: InstallMode) -> Result<()> {
    let target = root.join("codex-launcher.exe");
    for link in [
        shortcut::link_path(mode)?,
        shortcut::legacy_link_path(mode)?,
    ]
    .into_iter()
    .flatten()
    {
        if shortcut::is_owned(&link, &target, "")? {
            shortcut::remove(&link)?;
        }
    }
    Ok(())
}

pub fn create_or_update_desktop_shortcut(
    root: &Path,
    mode: InstallMode,
    use_current_junction: bool,
) -> Result<()> {
    let link = shortcut::desktop_link_path(mode)?;
    let legacy_link = shortcut::legacy_desktop_link_path(mode)?;
    let target = root.join("codex-launcher.exe");
    if link.exists() && !shortcut::is_owned(&link, &target, "--launch-latest")? {
        anyhow::bail!("桌面已存在其他来源的 ChatGPT 快捷方式，为避免覆盖已保留原文件");
    }
    let remove_legacy = shortcut::is_owned(&legacy_link, &target, "--launch-latest")?;
    let launch_target = crate::versions::resolve_launch_target(root, use_current_junction, None)?;
    let description = format!("启动最新 {}", launch_target.app_kind.display_name());
    shortcut::create_or_update(
        &link,
        &target,
        &launch_target.executable,
        &description,
        root,
        "--launch-latest",
    )?;
    if remove_legacy {
        shortcut::remove(&legacy_link)?;
    }
    Ok(())
}

pub fn desktop_shortcut_exists(root: &Path, mode: InstallMode) -> Result<bool> {
    let target = root.join("codex-launcher.exe");
    for link in [
        shortcut::desktop_link_path(mode)?,
        shortcut::legacy_desktop_link_path(mode)?,
    ] {
        if shortcut::is_owned(&link, &target, "--launch-latest")? {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn remove_desktop_shortcut(root: &Path, mode: InstallMode) -> Result<()> {
    let target = root.join("codex-launcher.exe");
    for link in [
        shortcut::desktop_link_path(mode)?,
        shortcut::legacy_desktop_link_path(mode)?,
    ] {
        if shortcut::is_owned(&link, &target, "--launch-latest")? {
            shortcut::remove(&link)?;
        }
    }
    Ok(())
}

pub fn create_or_update_assistant_desktop_shortcut(root: &Path, mode: InstallMode) -> Result<()> {
    let link = shortcut::assistant_desktop_link_path(mode)?;
    let target = root.join("codex-launcher.exe");
    if link.exists() && !shortcut::is_owned(&link, &target, "")? {
        anyhow::bail!(
            "桌面已存在其他来源的 Codex Windows 中文助手快捷方式，为避免覆盖已保留原文件"
        );
    }
    shortcut::create_or_update(
        &link,
        &target,
        &target,
        "打开 Codex Windows 中文助手",
        root,
        "",
    )
}

pub fn assistant_desktop_shortcut_exists(root: &Path, mode: InstallMode) -> Result<bool> {
    let target = root.join("codex-launcher.exe");
    shortcut::is_owned(&shortcut::assistant_desktop_link_path(mode)?, &target, "")
}

pub fn remove_assistant_desktop_shortcut(root: &Path, mode: InstallMode) -> Result<()> {
    let link = shortcut::assistant_desktop_link_path(mode)?;
    let target = root.join("codex-launcher.exe");
    if shortcut::is_owned(&link, &target, "")? {
        shortcut::remove(&link)?;
    }
    Ok(())
}

/// (Re)write the Add/Remove Programs registry entry for the current install.
fn write_registry(root: &Path, mode: InstallMode, version: &str) -> Result<()> {
    let launcher = root.join("codex-launcher.exe");
    let entry = registry::UninstallEntry {
        display_name: "Codex Windows 中文助手",
        display_version: version,
        publisher: "chrichuang218",
        install_location: root,
        uninstall_string: format!("\"{}\" --uninstall", launcher.display()),
        display_icon: &launcher,
    };
    registry::write(mode, &entry)
}

fn emit_progress_detail(
    on_msg: &dyn Fn(InstallMsg) -> Result<()>,
    phase: &str,
    detail: &str,
    progress: Option<f32>,
) -> Result<()> {
    on_msg(InstallMsg::ProgressDetail {
        phase: phase.into(),
        detail: detail.into(),
        progress,
    })
}

fn download_with_retry(
    fetcher: Fetcher,
    on_msg: &dyn Fn(InstallMsg) -> Result<()>,
    cancel: Option<&AtomicBool>,
    mut download: impl FnMut(&mut dyn FnMut(u64, Option<u64>) -> Result<()>) -> Result<DownloadResult>,
) -> Result<DownloadResult> {
    let max_attempts = if fetcher == Fetcher::Direct {
        DIRECT_DOWNLOAD_ATTEMPTS
    } else {
        1
    };
    let boxed = BoxedFn(on_msg, cancel);
    let mut attempt = 1;
    loop {
        check_cancel(cancel)?;
        let direct_attempt = (fetcher == Fetcher::Direct).then_some(attempt);
        if direct_attempt.is_some() {
            emit_progress_detail(
                on_msg,
                "Downloading",
                &format!("正在获取官方下载地址（第 {attempt}/{max_attempts} 次尝试）。"),
                Some(0.0),
            )?;
        }
        check_cancel(cancel)?;
        // Invoke the entire download again so each attempt gets a fresh signed URL.
        let result = download(&mut |done, total| boxed.progress_bytes(done, total, direct_attempt));
        check_cancel(cancel)?;
        let error = match result {
            Ok(result) => return Ok(result),
            Err(error) => error,
        };
        if error.to_string() == "安装已取消" || fetcher != Fetcher::Direct {
            return Err(error);
        }
        if attempt == max_attempts || !is_retryable_download_error(&error) {
            return Err(error)
                .with_context(|| format!("直连下载失败（已尝试 {attempt}/{max_attempts} 次）"));
        }
        emit_progress_detail(
            on_msg,
            "Downloading",
            &format!(
                "下载暂时失败，{attempt} 秒后重试（第 {}/{max_attempts} 次尝试）。",
                attempt + 1
            ),
            Some(0.0),
        )?;
        for _ in 0..attempt * 10 {
            check_cancel(cancel)?;
            std::thread::sleep(Duration::from_millis(100));
        }
        attempt += 1;
    }
}

fn is_retryable_download_error(error: &anyhow::Error) -> bool {
    // TLS validation failures can also be classified as connection errors.
    if error.chain().any(|cause| {
        cause
            .to_string()
            .to_ascii_lowercase()
            .contains("certificate")
    }) {
        return false;
    }
    error.chain().any(|cause| {
        let request_error = cause.downcast_ref::<reqwest::Error>().or_else(|| {
            // blocking Response::read wraps reqwest errors in std::io::Error.
            cause
                .downcast_ref::<std::io::Error>()
                .and_then(std::io::Error::get_ref)
                .and_then(|inner| inner.downcast_ref::<reqwest::Error>())
        });
        let Some(request_error) = request_error else {
            return false;
        };
        if let Some(status) = request_error.status() {
            return matches!(status.as_u16(), 408 | 429 | 500 | 502 | 503 | 504);
        }
        if request_error.is_timeout() || request_error.is_connect() {
            return true;
        }
        // Request/Body/Decode can wrap both transport and content failures.
        // Inspect their causes so malformed HTTP/JSON remains non-retryable.
        let mut source = std::error::Error::source(request_error);
        while let Some(cause) = source {
            if let Some(http) = cause.downcast_ref::<hyper::Error>() {
                if http.is_incomplete_message() || http.is_closed() {
                    return true;
                }
            }
            if let Some(io) = cause.downcast_ref::<std::io::Error>() {
                if matches!(
                    io.kind(),
                    std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::ConnectionAborted
                        | std::io::ErrorKind::BrokenPipe
                        | std::io::ErrorKind::UnexpectedEof
                        | std::io::ErrorKind::TimedOut
                ) {
                    return true;
                }
            }
            source = cause.source();
        }
        false
    })
}

fn download_start_detail(fetcher: Fetcher) -> String {
    match fetcher {
        Fetcher::Direct => "正在解析官方下载地址。".into(),
        Fetcher::Winget => "正在调用 winget 下载 Codex。".into(),
        Fetcher::LocalFile => "正在复制本地 MSIX 文件。".into(),
    }
}

fn configured_update_fetcher(fetcher: Fetcher) -> Fetcher {
    match fetcher {
        Fetcher::LocalFile => Fetcher::Direct,
        other => other,
    }
}

/// Adapt progress counters to messages while propagating cancellation and
/// progress-channel failures to the installer.
struct BoxedFn<'a>(&'a dyn Fn(InstallMsg) -> Result<()>, Option<&'a AtomicBool>);

impl BoxedFn<'_> {
    fn progress_bytes(&self, done: u64, total: Option<u64>, attempt: Option<u32>) -> Result<()> {
        check_cancel(self.1)?;
        let frac = total
            .filter(|t| *t > 0)
            .map(|t| (done as f32 / t as f32).clamp(0.0, 1.0));
        let detail = match total {
            Some(t) => format!("{} / {} MB", done / 1_048_576, t / 1_048_576),
            None => format!("{} MB", done / 1_048_576),
        };
        let detail = match attempt {
            Some(attempt) => format!("第 {attempt}/{DIRECT_DOWNLOAD_ATTEMPTS} 次尝试 · {detail}"),
            None => detail,
        };
        (self.0)(InstallMsg::ProgressDetail {
            phase: "Downloading".into(),
            detail,
            progress: frac,
        })
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
        })
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn response_error(response: &'static [u8]) -> anyhow::Error {
        response_error_with(response, |mut response| {
            std::io::copy(&mut response, &mut std::io::sink())?;
            Ok(())
        })
    }

    fn response_error_with(
        response: &'static [u8],
        read: impl FnOnce(reqwest::blocking::Response) -> Result<()>,
    ) -> anyhow::Error {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/fixture", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 4096];
            while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                let n = socket.read(&mut buffer).unwrap();
                assert_ne!(n, 0);
                request.extend_from_slice(&buffer[..n]);
            }
            socket.write_all(response).unwrap();
        });
        let result = (|| -> Result<()> {
            let client = reqwest::blocking::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(2))
                .build()?;
            read(client.get(url).send()?.error_for_status()?)
        })();
        server.join().unwrap();
        result.expect_err("fixture must fail")
    }

    fn temporary_server_error() -> anyhow::Error {
        response_error(
            b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        )
    }

    fn downloaded_package() -> DownloadResult {
        DownloadResult {
            msix_path: PathBuf::from("test-package.msix"),
            moniker: "OpenAI.Codex_1.2.3.0_x64__test".into(),
            version: "1.2.3.0".into(),
        }
    }

    #[test]
    fn direct_download_recovers_on_third_attempt_and_reports_fresh_progress() {
        let mut failures = VecDeque::from([
            temporary_server_error(),
            response_error(
                b"HTTP/1.1 200 OK\r\nContent-Length: 1024\r\nConnection: close\r\n\r\npartial",
            ),
        ]);
        let attempts = Cell::new(0);
        let events = RefCell::new(Vec::new());
        let result = download_with_retry(
            Fetcher::Direct,
            &|msg| {
                events.borrow_mut().push(msg);
                Ok(())
            },
            None,
            |progress| {
                attempts.set(attempts.get() + 1);
                progress(6, Some(10))?;
                match failures.pop_front() {
                    Some(error) => Err(error),
                    None => Ok(downloaded_package()),
                }
            },
        )
        .expect("third attempt should succeed");
        assert_eq!(result.version, "1.2.3.0");
        assert_eq!(attempts.get(), 3);
        let events = events.borrow();
        for attempt in 1..=3 {
            let label = format!("第 {attempt}/3 次尝试");
            assert!(
                events.iter().any(|msg| matches!(msg,
                    InstallMsg::ProgressDetail { detail, progress: Some(value), .. }
                    if detail.contains(&label) && *value == 0.0
                )),
                "must reset progress for {label}"
            );
            assert!(
                events.iter().any(|msg| matches!(msg,
                    InstallMsg::ProgressDetail { detail, progress: Some(value), .. }
                    if detail.contains(&label) && *value == 0.6
                )),
                "must preserve attempt label while receiving bytes"
            );
        }
    }

    #[test]
    fn direct_download_stops_after_three_failures_and_preserves_the_last_cause() {
        let mut attempts = 0;
        let error = download_with_retry(Fetcher::Direct, &|_| Ok(()), None, |_| {
            attempts += 1;
            Err(temporary_server_error())
        })
        .unwrap_err();
        assert_eq!(attempts, 3);
        assert!(format!("{error:#}").contains("已尝试 3/3 次"));
        assert!(format!("{error:#}").contains("503 Service Unavailable"));
    }

    #[test]
    fn direct_download_retries_connections_closed_before_response_headers_finish() {
        let mut failures = VecDeque::from([
            response_error(b""),
            response_error(b"HTTP/1.1 200 OK\r\nContent-Len"),
        ]);
        let mut attempts = 0;
        let result = download_with_retry(Fetcher::Direct, &|_| Ok(()), None, |_| {
            attempts += 1;
            match failures.pop_front() {
                Some(error) => Err(error),
                None => Ok(downloaded_package()),
            }
        });
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(attempts, 3);
    }

    #[test]
    fn direct_download_retries_truncated_text_and_json_metadata() {
        const TRUNCATED: &[u8] =
            b"HTTP/1.1 200 OK\r\nContent-Length: 1024\r\nConnection: close\r\n\r\n{}";
        let mut failures = VecDeque::from([
            response_error_with(TRUNCATED, |response| {
                response.text()?;
                Ok(())
            }),
            response_error_with(TRUNCATED, |response| {
                response.json::<serde_json::Value>()?;
                Ok(())
            }),
        ]);
        let mut attempts = 0;
        let result = download_with_retry(Fetcher::Direct, &|_| Ok(()), None, |_| {
            attempts += 1;
            match failures.pop_front() {
                Some(error) => Err(error),
                None => Ok(downloaded_package()),
            }
        });
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(attempts, 3);
    }

    #[test]
    fn direct_download_retries_network_timeouts() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/timeout", listener.local_addr().unwrap());
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(Duration::from_millis(25))
            .build()
            .unwrap();
        let mut attempts = 0;
        let error = download_with_retry(Fetcher::Direct, &|_| Ok(()), None, |_| {
            attempts += 1;
            // Keep the listener open without responding to any request.
            Err(client.get(&url).send().unwrap_err().into())
        })
        .unwrap_err();
        assert_eq!(attempts, 3);
        assert!(format!("{error:#}").contains("timed out"));
    }

    #[test]
    fn permanent_errors_and_cancellation_are_not_retried() {
        let errors = [
            response_error(
                b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            ),
            anyhow::Error::from(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "disk write denied",
            )),
            anyhow::anyhow!("安装已取消"),
            temporary_server_error().context("invalid peer certificate"),
            anyhow::anyhow!("Parsing SyncUpdates response failed"),
            response_error(b"THIS IS NOT HTTP\r\n\r\n"),
            response_error(
                b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\nZ\r\ninvalid\r\n0\r\n\r\n",
            ),
            response_error_with(
                b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\nnotjson",
                |response| {
                    response.json::<serde_json::Value>()?;
                    Ok(())
                },
            ),
        ];
        for error in errors {
            let raw = format!("{error:#}");
            let mut error = Some(error);
            let mut attempts = 0;
            let actual = download_with_retry(Fetcher::Direct, &|_| Ok(()), None, |_| {
                attempts += 1;
                Err(error.take().expect("permanent error must not be retried"))
            })
            .unwrap_err();
            assert_eq!(attempts, 1);
            assert!(format!("{actual:#}").contains(&raw));
            if raw == "安装已取消" {
                assert_eq!(actual.to_string(), raw);
            }
        }
    }

    #[test]
    fn cancelling_during_retry_wait_does_not_start_another_request() {
        let cancel = AtomicBool::new(false);
        let mut attempts = 0;
        let error = download_with_retry(Fetcher::Direct, &|msg| {
            if matches!(msg, InstallMsg::ProgressDetail { detail, .. } if detail.contains("秒后重试")) {
                cancel.store(true, Ordering::SeqCst);
            }
            Ok(())
        }, Some(&cancel), |_| {
            attempts += 1;
            Err(temporary_server_error())
        }).unwrap_err();
        assert_eq!(attempts, 1);
        assert_eq!(error.to_string(), "安装已取消");
    }

    #[test]
    fn manually_selected_winget_is_not_automatically_retried() {
        let mut attempts = 0;
        let error = download_with_retry(Fetcher::Winget, &|_| Ok(()), None, |_| {
            attempts += 1;
            Err(temporary_server_error())
        })
        .unwrap_err();
        assert_eq!(attempts, 1);
        assert!(format!("{error:#}").contains("503 Service Unavailable"));
    }

    #[test]
    fn update_keeps_configured_direct_fetcher() {
        assert_eq!(configured_update_fetcher(Fetcher::Direct), Fetcher::Direct);
    }

    #[test]
    fn progress_channel_failure_stops_before_starting_a_download() {
        let mut requests = 0;
        let error = download_with_retry(
            Fetcher::Direct,
            &|_| anyhow::bail!("回传管理员任务进度失败"),
            None,
            |_| {
                requests += 1;
                Ok(downloaded_package())
            },
        )
        .unwrap_err();
        assert_eq!(requests, 0);
        assert_eq!(error.to_string(), "回传管理员任务进度失败");
    }

    #[test]
    fn update_keeps_configured_winget_fetcher() {
        assert_eq!(configured_update_fetcher(Fetcher::Winget), Fetcher::Winget);
    }

    #[test]
    fn update_normalizes_legacy_local_file_fetcher_to_direct() {
        assert_eq!(
            configured_update_fetcher(Fetcher::LocalFile),
            Fetcher::Direct
        );
    }

    #[test]
    fn update_direct_detail_matches_actual_preferred_fetcher() {
        assert_eq!(
            download_start_detail(configured_update_fetcher(Fetcher::Direct)),
            "正在解析官方下载地址。"
        );
    }
}

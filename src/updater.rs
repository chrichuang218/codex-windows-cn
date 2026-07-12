//! Update check + defer-decision helpers.
//!
//! The update check is cheap: it resolves the Store's latest version string
//! via FE3 SyncUpdates (no download) and compares against `config.current_version`.
//! Callers apply the user's response by mutating the Config and saving it.
//!
//! Decision flow:
//!   1. Policy == Never          → Skipped
//!   2. suppress_until_unix > now → Skipped
//!   3. Resolve latest
//!      - err                     → Error (show but let user continue)
//!      - == current              → UpToDate
//!      - != current              → Available { current, latest }
//!
//! The policy (Always/Daily/Weekly) only gates *automatic* checks from proxy
//! mode — an explicit "Check for updates" action bypasses it.

use crate::config::{Config, UpdatePolicy};
use crate::store::{self, Fetcher};
use std::time::{SystemTime, UNIX_EPOCH};

/// Owner/repo for the launcher's own GitHub releases. Tweak here if the
/// project ever moves.
pub const LAUNCHER_REPO: &str = "chrichuang218/codex-windows-cn";
pub const LAUNCHER_LATEST_RELEASE_URL: &str =
    "https://github.com/chrichuang218/codex-windows-cn/releases/latest";
pub const LAUNCHER_LATEST_API: &str =
    "https://api.github.com/repos/chrichuang218/codex-windows-cn/releases/latest";

#[derive(Debug, Clone)]
pub enum UpdateDecision {
    /// Skip the check entirely (policy=Never, suppressed, or policy cooldown).
    Skipped { reason: String },
    /// Check ran; we're on the latest version.
    UpToDate {
        version: String,
        product_name: String,
    },
    /// Check ran; a newer version exists.
    Available {
        current: String,
        latest: String,
        product_name: String,
    },
    /// Check failed — surface the error but don't block the app.
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeferChoice {
    /// Update right now.
    UpdateNow,
    /// Remind me on the next scheduled check.
    NotNow,
    /// Skip this specific version forever.
    SkipThisVersion,
    /// Snooze 1 day.
    SnoozeOneDay,
    /// Snooze 7 days.
    SnoozeSevenDays,
    /// Turn off all update prompts.
    Never,
}

/// Automatic check — honors policy cooldown + suppress_until. Use this from
/// proxy-mode startup.
pub fn check_auto(cfg: &Config, product_id: &str) -> UpdateDecision {
    if let Some(reason) = auto_check_skip_reason(cfg) {
        return UpdateDecision::Skipped { reason };
    }
    let decision = check_now(cfg, product_id);
    // Honor "skip this version" only while the Store's latest still matches
    // the skipped version — once Microsoft publishes something newer, the
    // suppression is implicitly lifted.
    if let UpdateDecision::Available { latest, .. } = &decision {
        if cfg.skipped_version.as_deref() == Some(latest.as_str()) {
            return UpdateDecision::Skipped {
                reason: format!("version {latest} skipped by user"),
            };
        }
    }
    decision
}

/// True when `check_auto` will perform the Store version lookup instead of
/// skipping immediately due to policy, snooze, or cooldown.
pub fn auto_check_will_query(cfg: &Config) -> bool {
    auto_check_skip_reason(cfg).is_none()
}

/// Reuse the last successful Store result while an automatic check is inside
/// its cooldown. This keeps a known update visible without performing a new
/// network request.
pub fn cached_update_decision(cfg: &Config, product_name: &str) -> Option<UpdateDecision> {
    if cfg.update_policy == UpdatePolicy::Never {
        return None;
    }
    if cfg
        .suppress_until_unix
        .is_some_and(|until| now_unix() < until)
    {
        return None;
    }
    let latest = cfg.known_latest.as_ref()?;
    if cfg.skipped_version.as_deref() == Some(latest.as_str()) {
        return None;
    }
    if version_gt(latest, &cfg.current_version) {
        return Some(UpdateDecision::Available {
            current: cfg.current_version.clone(),
            latest: latest.clone(),
            product_name: product_name.to_string(),
        });
    }
    Some(UpdateDecision::UpToDate {
        version: cfg.current_version.clone(),
        product_name: product_name.to_string(),
    })
}

/// Force a check regardless of policy/suppression. Use this when the user
/// explicitly clicks "Check for updates".
pub fn check_now(cfg: &Config, product_id: &str) -> UpdateDecision {
    match store::resolve_latest_product(cfg.fetcher, product_id) {
        Ok(product) => {
            if version_gt(&product.version, &cfg.current_version) {
                UpdateDecision::Available {
                    current: cfg.current_version.clone(),
                    latest: product.version,
                    product_name: product.title,
                }
            } else {
                UpdateDecision::UpToDate {
                    version: product.version,
                    product_name: product.title,
                }
            }
        }
        Err(e) => UpdateDecision::Error(format!("{:#}", e)),
    }
}

/// Apply a defer choice to `cfg`. Caller is responsible for saving afterward.
/// `latest` is the version the user was prompted about (used for SkipThisVersion).
pub fn apply_defer(cfg: &mut Config, choice: DeferChoice, latest: &str) {
    let now = now_unix();
    cfg.last_check_unix = Some(now);
    cfg.known_latest = Some(latest.to_string());
    match choice {
        DeferChoice::UpdateNow => {
            cfg.suppress_until_unix = None;
            cfg.skipped_version = None;
        }
        DeferChoice::NotNow => {
            // Let normal policy cooldown govern the next check — nothing to do.
        }
        DeferChoice::SkipThisVersion => {
            // Suppress prompts specifically for this version. `check_auto`
            // filters Available decisions where latest == skipped_version;
            // once the Store moves past this version, prompts resume.
            cfg.skipped_version = Some(latest.to_string());
        }
        DeferChoice::SnoozeOneDay => cfg.suppress_until_unix = Some(now + 86_400),
        DeferChoice::SnoozeSevenDays => cfg.suppress_until_unix = Some(now + 7 * 86_400),
        DeferChoice::Never => {
            cfg.update_policy = UpdatePolicy::Never;
            cfg.suppress_until_unix = None;
        }
    }
}

/// Record a successful up-to-date check — bumps `last_check_unix` + known_latest.
pub fn record_check(cfg: &mut Config, latest: &str) {
    cfg.last_check_unix = Some(now_unix());
    cfg.known_latest = Some(latest.to_string());
}

/// Record an automatic check attempt so cooldown policies also apply after an
/// available update or a transient error.
pub fn record_auto_check(cfg: &mut Config, decision: &UpdateDecision) {
    cfg.last_check_unix = Some(now_unix());
    match decision {
        UpdateDecision::Available { latest, .. } => cfg.known_latest = Some(latest.clone()),
        UpdateDecision::UpToDate { version, .. } => cfg.known_latest = Some(version.clone()),
        UpdateDecision::Skipped { .. } | UpdateDecision::Error(_) => {}
    }
}

// -- Launcher self-update check -------------------------------------------

#[derive(Debug, Clone)]
#[allow(dead_code)] // Skipped/Error fields are reserved for future logging.
pub enum LauncherDecision {
    /// Skipped (policy=Never, snoozed, within cooldown, or skip-this-version match).
    Skipped { reason: String },
    /// We're on the latest tag.
    UpToDate { version: String },
    /// A newer tag is published. `release_url` is the GitHub Releases page.
    Available {
        current: String,
        latest: String,
        release_url: String,
    },
    /// API call or parse failed. Surface but don't block.
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LauncherDeferChoice {
    /// Open the release page in the user's default browser. Doesn't dismiss.
    ViewRelease,
    /// Defer until next cooldown roll.
    NotNow,
    /// Suppress prompts only while GitHub's latest equals this version.
    SkipThisVersion,
    SnoozeOneDay,
    SnoozeSevenDays,
    /// Suppress launcher prompts effectively forever (suppress_until = u64::MAX).
    Never,
    /// Download the new launcher and replace the running exe in place.
    /// Caller drives the download/swap; this variant is just the action signal.
    ApplyUpdate,
}

/// Reconstruct a pending launcher prompt from persisted state — no network.
/// Used by paths that can't re-run the bg check (e.g. elevated `--auto-update`
/// re-spawn — the unelevated process already bumped the shared cooldown).
/// Returns `Some` only if `known_latest_launcher` is newer than the running
/// version and the user hasn't silenced it (skip / snooze / never).
pub fn pending_launcher_from_state(cfg: &Config) -> Option<LauncherDecision> {
    if cfg.update_policy == UpdatePolicy::Never {
        return None;
    }
    if let Some(until) = cfg.launcher_suppress_until_unix {
        if now_unix() < until {
            return None;
        }
    }
    let latest = cfg.known_latest_launcher.as_ref()?;
    if cfg.skipped_launcher_version.as_deref() == Some(latest.as_str()) {
        return None;
    }
    let current = env!("CARGO_PKG_VERSION").to_string();
    if !version_gt(latest, &current) {
        return None;
    }
    Some(LauncherDecision::Available {
        current,
        latest: latest.clone(),
        release_url: format!("https://github.com/{LAUNCHER_REPO}/releases/tag/v{latest}"),
    })
}

/// Automatic launcher-update check. Honors the shared `update_policy` and
/// its own cooldown timestamp, plus launcher-specific snooze and
/// skip-this-version.
pub fn check_launcher_auto(cfg: &Config) -> LauncherDecision {
    if let Some(reason) = launcher_auto_check_skip_reason(cfg) {
        return LauncherDecision::Skipped { reason };
    }

    let decision = check_launcher_now();
    if let LauncherDecision::Available { latest, .. } = &decision {
        if cfg.skipped_launcher_version.as_deref() == Some(latest.as_str()) {
            return LauncherDecision::Skipped {
                reason: format!("launcher version {latest} skipped by user"),
            };
        }
    }
    decision
}

/// True when `check_launcher_auto` will call GitHub instead of skipping
/// immediately due to policy, launcher snooze, or shared cooldown.
pub fn launcher_auto_check_will_query(cfg: &Config) -> bool {
    launcher_auto_check_skip_reason(cfg).is_none()
}

/// Force a launcher-update check regardless of policy/snooze/cooldown.
pub fn check_launcher_now() -> LauncherDecision {
    let current = env!("CARGO_PKG_VERSION").to_string();
    match fetch_latest_launcher_tag() {
        Ok(tag) => {
            // Strip "v" prefix if present (we tag releases as v0.1.0 but
            // CARGO_PKG_VERSION is plain 0.1.0).
            let latest_ver = tag.trim_start_matches('v').to_string();
            if version_gt(&latest_ver, &current) {
                LauncherDecision::Available {
                    current,
                    latest: latest_ver,
                    release_url: format!("https://github.com/{LAUNCHER_REPO}/releases/tag/{tag}"),
                }
            } else {
                LauncherDecision::UpToDate {
                    version: latest_ver,
                }
            }
        }
        Err(e) => LauncherDecision::Error(format!("{e:#}")),
    }
}

fn fetch_latest_launcher_tag() -> anyhow::Result<String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent(concat!("codex-windows-updater/", env!("CARGO_PKG_VERSION")))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;

    fetch_latest_launcher_tag_from_redirect(&client).or_else(|redirect_error| {
        fetch_latest_launcher_tag_from_api(&client).map_err(|api_error| {
            anyhow::anyhow!(
                "GitHub release redirect check failed: {redirect_error:#}; \
                 GitHub API check failed: {api_error:#}"
            )
        })
    })
}

fn fetch_latest_launcher_tag_from_redirect(
    client: &reqwest::blocking::Client,
) -> anyhow::Result<String> {
    let resp = client.get(LAUNCHER_LATEST_RELEASE_URL).send()?;
    if !resp.status().is_redirection() {
        anyhow::bail!(
            "expected release redirect from {LAUNCHER_LATEST_RELEASE_URL}, got {}",
            resp.status()
        );
    }

    let location = resp
        .headers()
        .get(reqwest::header::LOCATION)
        .ok_or_else(|| anyhow::anyhow!("release redirect missing Location header"))?
        .to_str()?;

    tag_from_latest_release_location(location)
        .ok_or_else(|| anyhow::anyhow!("release redirect Location has no tag: {location}"))
}

fn fetch_latest_launcher_tag_from_api(
    client: &reqwest::blocking::Client,
) -> anyhow::Result<String> {
    use serde::Deserialize;
    #[derive(Deserialize)]
    struct LatestRelease {
        tag_name: String,
    }
    let resp = client
        .get(LAUNCHER_LATEST_API)
        .header("Accept", "application/vnd.github+json")
        .send()?
        .error_for_status()?;
    let body: LatestRelease = resp.json()?;
    Ok(body.tag_name)
}

fn tag_from_latest_release_location(location: &str) -> Option<String> {
    let without_fragment = location.split('#').next()?;
    let without_query = without_fragment.split('?').next()?;
    let marker = "/releases/tag/";
    let start = without_query.find(marker)? + marker.len();
    let tag = without_query[start..].split('/').next()?.trim();
    if tag.is_empty() {
        return None;
    }
    Some(tag.to_string())
}

/// Apply a launcher defer choice. Caller persists.
pub fn apply_launcher_defer(cfg: &mut Config, choice: LauncherDeferChoice, latest: &str) {
    let now = now_unix();
    cfg.known_latest_launcher = Some(latest.to_string());
    match choice {
        LauncherDeferChoice::ViewRelease
        | LauncherDeferChoice::NotNow
        | LauncherDeferChoice::ApplyUpdate => {
            // No state change beyond known_latest_launcher. Cooldown
            // governs next prompt. ApplyUpdate is a no-op here — the
            // self-update worker mutates state on success.
        }
        LauncherDeferChoice::SkipThisVersion => {
            cfg.skipped_launcher_version = Some(latest.to_string());
        }
        LauncherDeferChoice::SnoozeOneDay => {
            cfg.launcher_suppress_until_unix = Some(now + 86_400);
        }
        LauncherDeferChoice::SnoozeSevenDays => {
            cfg.launcher_suppress_until_unix = Some(now + 7 * 86_400);
        }
        LauncherDeferChoice::Never => {
            cfg.launcher_suppress_until_unix = Some(u64::MAX);
        }
    }
}

/// Record that the launcher update check ran. Successful checks also refresh
/// the latest launcher version cache; errors only advance the cooldown so a
/// transient GitHub limit does not get hammered on every app launch.
pub fn record_launcher_check(cfg: &mut Config, decision: &LauncherDecision) {
    cfg.last_launcher_check_unix = Some(now_unix());
    match decision {
        LauncherDecision::Available { latest, .. } => {
            cfg.known_latest_launcher = Some(latest.clone());
        }
        LauncherDecision::UpToDate { version } => {
            cfg.known_latest_launcher = Some(version.clone());
        }
        LauncherDecision::Skipped { .. } | LauncherDecision::Error(_) => {}
    }
}

fn policy_cooldown_secs(p: UpdatePolicy) -> u64 {
    match p {
        UpdatePolicy::Always => 0,
        UpdatePolicy::Daily => 86_400,
        UpdatePolicy::Weekly => 7 * 86_400,
        UpdatePolicy::Never => u64::MAX, // unreachable (filtered earlier)
    }
}

fn auto_check_skip_reason(cfg: &Config) -> Option<String> {
    let now = now_unix();
    if cfg.update_policy == UpdatePolicy::Never {
        return Some("update_policy = never".into());
    }
    if let Some(until) = cfg.suppress_until_unix {
        if now < until {
            let days = (until - now) / 86_400;
            return Some(format!("suppressed for ~{days}d"));
        }
    }
    if let Some(last) = cfg.last_check_unix {
        let cooldown = policy_cooldown_secs(cfg.update_policy);
        if now.saturating_sub(last) < cooldown {
            return Some("within cooldown".into());
        }
    }
    None
}

fn launcher_auto_check_skip_reason(cfg: &Config) -> Option<String> {
    let now = now_unix();
    if cfg.update_policy == UpdatePolicy::Never {
        return Some("update_policy = never".into());
    }
    if let Some(until) = cfg.launcher_suppress_until_unix {
        if now < until {
            let days = (until - now) / 86_400;
            return Some(format!("launcher prompt suppressed for ~{days}d"));
        }
    }
    if let Some(last) = cfg.last_launcher_check_unix {
        let cooldown = policy_cooldown_secs(cfg.update_policy);
        if now.saturating_sub(last) < cooldown {
            return Some("within cooldown".into());
        }
    }
    None
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Dotted-numeric compare. `a > b`?
fn version_gt(a: &str, b: &str) -> bool {
    let pa: Vec<u64> = a.split('.').map(|p| p.parse().unwrap_or(0)).collect();
    let pb: Vec<u64> = b.split('.').map(|p| p.parse().unwrap_or(0)).collect();
    pa > pb
}

// Keep this so the `Fetcher` import stays live if we later gate by it.
#[allow(dead_code)]
fn _fetcher_check(f: Fetcher) -> Fetcher {
    f
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::InstallMode;

    #[test]
    fn version_compare() {
        assert!(version_gt("26.500.0.0", "26.422.2437.0"));
        assert!(!version_gt("26.422.2437.0", "26.422.2437.0"));
        assert!(!version_gt("26.100.0.0", "26.422.0.0"));
        assert!(version_gt("27.0.0.0", "26.999.999.999"));
    }

    #[test]
    fn latest_release_redirect_location_yields_tag() {
        assert_eq!(
            tag_from_latest_release_location(
                "https://github.com/chrichuang218/codex-windows-cn/releases/tag/v0.2.0"
            )
            .as_deref(),
            Some("v0.2.0")
        );
        assert_eq!(
            tag_from_latest_release_location(
                "/chrichuang218/codex-windows-cn/releases/tag/v0.2.1?expanded=true"
            )
            .as_deref(),
            Some("v0.2.1")
        );
        assert_eq!(tag_from_latest_release_location("/releases/latest"), None);
    }

    #[test]
    fn automatic_check_frequency_honors_policy_and_cooldown() {
        let mut cfg = test_config();

        cfg.update_policy = UpdatePolicy::Never;
        assert!(!auto_check_will_query(&cfg));

        cfg.update_policy = UpdatePolicy::Daily;
        cfg.last_check_unix = Some(u64::MAX);
        assert!(!auto_check_will_query(&cfg));

        cfg.update_policy = UpdatePolicy::Weekly;
        assert!(!auto_check_will_query(&cfg));

        cfg.update_policy = UpdatePolicy::Always;
        assert!(auto_check_will_query(&cfg));
    }

    #[test]
    fn cached_available_update_is_reused_during_cooldown() {
        let mut cfg = test_config();
        cfg.known_latest = Some("2.0.0".into());
        cfg.last_check_unix = Some(u64::MAX);

        assert!(matches!(
            cached_update_decision(&cfg, "ChatGPT"),
            Some(UpdateDecision::Available {
                current,
                latest,
                product_name,
            }) if current == "1.0.0" && latest == "2.0.0" && product_name == "ChatGPT"
        ));
    }

    #[test]
    fn app_and_launcher_cooldowns_are_independent() {
        let mut cfg = test_config();
        cfg.last_check_unix = Some(u64::MAX);
        assert!(!auto_check_will_query(&cfg));
        assert!(launcher_auto_check_will_query(&cfg));

        cfg.last_check_unix = None;
        cfg.last_launcher_check_unix = Some(u64::MAX);
        assert!(auto_check_will_query(&cfg));
        assert!(!launcher_auto_check_will_query(&cfg));

        cfg.last_check_unix = None;
        cfg.last_launcher_check_unix = None;
        record_auto_check(&mut cfg, &UpdateDecision::Error("offline".into()));
        assert!(launcher_auto_check_will_query(&cfg));
        record_launcher_check(&mut cfg, &LauncherDecision::Error("offline".into()));
        assert!(!auto_check_will_query(&cfg));
        assert!(!launcher_auto_check_will_query(&cfg));
    }

    fn test_config() -> Config {
        Config {
            install_mode: InstallMode::User,
            current_version: "1.0.0".into(),
            update_policy: UpdatePolicy::Daily,
            last_check_unix: None,
            last_launcher_check_unix: None,
            suppress_until_unix: None,
            known_latest: None,
            skipped_version: None,
            keep_versions: 2,
            keep_all_versions: false,
            fetcher: Fetcher::Direct,
            use_current_junction: true,
            register_uninstall: true,
            register_codex_protocol: Some(true),
            known_latest_launcher: None,
            skipped_launcher_version: None,
            launcher_suppress_until_unix: None,
        }
    }
}

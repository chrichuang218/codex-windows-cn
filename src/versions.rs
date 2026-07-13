use crate::config::Config;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

const CACHE_FILENAME: &str = ".launcher-versions.json";

#[derive(Debug, Default, Deserialize, Serialize)]
struct VersionCache {
    entries: HashMap<String, CachedVersion>,
}

#[derive(Debug, Deserialize, Serialize)]
struct CachedVersion {
    directory_modified_unix: u64,
    installed_at_unix: u64,
    size_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AppKind {
    Codex,
    ChatGpt,
}

impl AppKind {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::ChatGpt => "ChatGPT",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledVersion {
    pub version: String,
    pub app_kind: AppKind,
    pub executable: PathBuf,
    pub size_bytes: u64,
    pub installed_at_unix: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchTarget {
    pub version: String,
    pub app_kind: AppKind,
    pub executable: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetentionPolicy {
    KeepLatest(u32),
    KeepAll,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteRepair {
    pub default_version: String,
    pub current_repaired: bool,
}

pub fn scan_installed(root: &Path) -> Result<Vec<InstalledVersion>> {
    let versions_root = root.join("versions");
    if !versions_root.is_dir() {
        return Ok(Vec::new());
    }

    let cache_path = versions_root.join(CACHE_FILENAME);
    let mut cache = load_cache(&cache_path);
    let mut next_cache = VersionCache::default();
    let mut cache_changed = false;
    let mut installed = Vec::new();
    for entry in fs::read_dir(&versions_root)
        .with_context(|| format!("reading {}", versions_root.display()))?
    {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if !file_type.is_dir() || file_type.is_symlink() {
            continue;
        }

        let version = entry.file_name().to_string_lossy().into_owned();
        if !is_version_name(&version) {
            continue;
        }

        let Some((app_kind, executable)) = detect_entrypoint(&entry.path()) else {
            continue;
        };
        let metadata = entry.metadata()?;
        let directory_modified_unix = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        let installed_at_unix = metadata
            .created()
            .or_else(|_| metadata.modified())
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs())
            .unwrap_or(directory_modified_unix);
        let cached = cache.entries.remove(&version);
        let size_bytes = match cached {
            Some(cached) if cached.directory_modified_unix == directory_modified_unix => {
                next_cache.entries.insert(
                    version.clone(),
                    CachedVersion {
                        directory_modified_unix,
                        installed_at_unix: cached.installed_at_unix,
                        size_bytes: cached.size_bytes,
                    },
                );
                cached.size_bytes
            }
            _ => {
                cache_changed = true;
                let size = directory_size(&entry.path())?;
                next_cache.entries.insert(
                    version.clone(),
                    CachedVersion {
                        directory_modified_unix,
                        installed_at_unix,
                        size_bytes: size,
                    },
                );
                size
            }
        };

        installed.push(InstalledVersion {
            version,
            app_kind,
            executable,
            size_bytes,
            installed_at_unix,
        });
    }

    if !cache.entries.is_empty() || next_cache.entries.len() != installed.len() {
        cache_changed = true;
    }
    if cache_changed {
        let _ = save_cache(&cache_path, &next_cache);
    }

    installed.sort_by(|left, right| {
        parse_version(&right.version)
            .expect("scanned version is valid")
            .cmp(&parse_version(&left.version).expect("scanned version is valid"))
    });
    Ok(installed)
}

pub fn resolve_launch_target(
    root: &Path,
    use_junction: bool,
    requested_version: Option<&str>,
) -> Result<LaunchTarget> {
    let installed = scan_installed(root)?;
    let selected = match requested_version {
        Some(version) => installed.iter().find(|item| item.version == version),
        None => installed.first(),
    }
    .ok_or_else(|| match requested_version {
        Some(version) => anyhow::anyhow!("installed version {version} is not available"),
        None => anyhow::anyhow!("no launchable installed version under {}", root.display()),
    })?;

    let executable = if requested_version.is_none() && use_junction {
        if let Err(error) = crate::junction::set_current(root, &selected.version) {
            eprintln!("warn: couldn't repair versions/current junction: {error:#}");
            selected.executable.clone()
        } else {
            let file_name = selected
                .executable
                .file_name()
                .ok_or_else(|| anyhow::anyhow!("launch executable has no file name"))?;
            root.join("versions").join("current").join(file_name)
        }
    } else {
        selected.executable.clone()
    };

    Ok(LaunchTarget {
        version: selected.version.clone(),
        app_kind: selected.app_kind,
        executable,
    })
}

pub fn versions_to_prune(
    installed: &[InstalledVersion],
    policy: RetentionPolicy,
    running_versions: &HashSet<String>,
) -> Vec<String> {
    let RetentionPolicy::KeepLatest(keep) = policy else {
        return Vec::new();
    };
    installed
        .iter()
        .skip(keep.max(1) as usize)
        .filter(|item| !running_versions.contains(&item.version))
        .map(|item| item.version.clone())
        .collect()
}

pub fn prune_installed(
    root: &Path,
    policy: RetentionPolicy,
    running_versions: &HashSet<String>,
) -> Result<Vec<String>> {
    let installed = scan_installed(root)?;
    let versions_root = root.join("versions");
    let to_remove = versions_to_prune(&installed, policy, running_versions);
    let mut removed = Vec::new();
    for version in to_remove {
        let path = versions_root.join(&version);
        fs::remove_dir_all(&path).with_context(|| format!("removing {}", path.display()))?;
        removed.push(version);
    }
    remove_partial_directories(&versions_root)?;
    Ok(removed)
}

pub fn delete_installed(
    root: &Path,
    version: &str,
    running_versions: &HashSet<String>,
) -> Result<()> {
    if !is_version_name(version) {
        bail!("invalid installed version: {version}");
    }
    if running_versions.contains(version) {
        bail!("version {version} is currently running");
    }

    let installed = scan_installed(root)?;
    if installed.len() <= 1 {
        bail!("cannot delete the final launchable version");
    }
    if !installed.iter().any(|item| item.version == version) {
        bail!("installed version {version} does not exist");
    }

    let path = root.join("versions").join(version);
    fs::remove_dir_all(&path).with_context(|| format!("removing {}", path.display()))
}

pub fn delete_and_repair(
    root: &Path,
    cfg: &mut Config,
    version: &str,
    running_versions: &HashSet<String>,
) -> Result<DeleteRepair> {
    delete_installed(root, version, running_versions)?;
    let installed = scan_installed(root)?;
    let latest = installed
        .first()
        .ok_or_else(|| anyhow::anyhow!("deletion left no launchable installed version"))?;

    cfg.current_version = latest.version.clone();
    cfg.save_runtime(root)?;
    let current_repaired =
        !cfg.use_current_junction || crate::junction::set_current(root, &latest.version).is_ok();

    Ok(DeleteRepair {
        default_version: latest.version.clone(),
        current_repaired,
    })
}

fn detect_entrypoint(version_dir: &Path) -> Option<(AppKind, PathBuf)> {
    let chatgpt = version_dir.join("ChatGPT.exe");
    if chatgpt.is_file() {
        return Some((AppKind::ChatGpt, chatgpt));
    }
    let codex = version_dir.join("Codex.exe");
    codex.is_file().then_some((AppKind::Codex, codex))
}

pub fn is_version_name(version: &str) -> bool {
    parse_version(version).is_some()
}

fn parse_version(version: &str) -> Option<Vec<u64>> {
    let parts: Option<Vec<u64>> = version
        .split('.')
        .map(|part| {
            if part.is_empty() {
                None
            } else {
                part.parse::<u64>().ok()
            }
        })
        .collect();
    parts.filter(|parts| parts.len() >= 2)
}

fn directory_size(path: &Path) -> Result<u64> {
    let mut total = 0u64;
    for entry in fs::read_dir(path).with_context(|| format!("reading {}", path.display()))? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            total = total.saturating_add(directory_size(&entry.path())?);
        } else if file_type.is_file() {
            total = total.saturating_add(entry.metadata()?.len());
        }
    }
    Ok(total)
}

fn load_cache(path: &Path) -> VersionCache {
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_cache(path: &Path, cache: &VersionCache) -> Result<()> {
    let temporary = path.with_extension("tmp");
    let raw = serde_json::to_vec_pretty(cache)?;
    fs::write(&temporary, raw).with_context(|| format!("writing {}", temporary.display()))?;
    if path.exists() {
        fs::remove_file(path).with_context(|| format!("removing {}", path.display()))?;
    }
    fs::rename(&temporary, path)
        .with_context(|| format!("renaming {} to {}", temporary.display(), path.display()))
}

fn remove_partial_directories(versions_root: &Path) -> Result<()> {
    if !versions_root.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(versions_root)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".partial") && entry.file_type()?.is_dir() {
            fs::remove_dir_all(entry.path())?;
        }
    }
    Ok(())
}

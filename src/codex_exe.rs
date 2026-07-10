use std::path::{Path, PathBuf};

/// Compatibility wrapper for callers that still use the original module name.
/// The selected executable may now be `ChatGPT.exe` or legacy `Codex.exe`.
pub fn latest_codex_exe(root: &Path, use_junction: bool) -> Option<PathBuf> {
    crate::versions::resolve_launch_target(root, use_junction, None)
        .ok()
        .map(|target| target.executable)
}

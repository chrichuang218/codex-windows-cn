# Local Version Manager Design

## Summary

The launcher becomes a local version manager for official Microsoft Store packages. It continues installing and updating only the latest official package, while retaining valid extracted versions for historical launch. The installed workspace becomes a compact operational console with Overview, Versions, and Settings views.

## Domain And Behavior

- A **local installed version** is a strict dotted-numeric directory under `versions/` with a validated launch entrypoint.
- App classification is content-based: `ChatGPT.exe` means ChatGPT; otherwise a valid `Codex.exe` means legacy Codex.
- The default launch target is always the newest valid installed version. Selecting an older version performs a one-time historical launch and does not change update policy.
- All versions share `%APPDATA%\Codex`. If a different managed version is running, the UI asks for confirmation, terminates the managed process tree, waits for exit, then starts the target.
- A running version and the only remaining valid version cannot be deleted. Other versions, including the newest, can be deleted; the newest remaining version becomes the default.
- Retention supports `keep latest N` and `keep all`. The default is five. Keep-all disables automatic version deletion; manual deletion remains available.

## Product Identity

- Official Microsoft Store `ProductTitle` drives the current product label shown by the assistant.
- Installed rows retain their own ChatGPT or Codex classification.
- The repository name, Tauri identifier, config paths, user-data directory, and uninstall identity remain stable.
- Content detection is authoritative; the observed transition at `26.707.3748.0` is a regression fixture, not a permanent hardcoded boundary.

## Interfaces And Data Flow

- Backend exposes an installed-version inventory containing version, app kind, executable, size, install time, latest/running state, and deletion eligibility.
- Launch requests accept an optional target version and a confirmed-switch flag. Conflicts return structured running/target information instead of an English native dialog.
- Delete requests accept a version and return the refreshed inventory/default version.
- Settings persist numeric retention plus an explicit keep-all flag for backward-compatible config migration.
- Update completion refreshes inventory. If an older version remains running, Overview shows a switch-to-latest action without interrupting work automatically.

## UX Direction

- Increase the desktop window to an 880x600 resizable operational console with a restrained navigation rail.
- Overview prioritizes current official product, running/default versions, update status, primary launch/switch action, and installed storage summary.
- Versions uses dense rows with app-kind badges, version, size, install date, state, launch, and delete actions.
- Settings contains retention controls, install location, download method, launcher update, and uninstall maintenance actions.
- Use native Windows typography, graphite surfaces, mint primary actions, lime success, amber warnings, and coral destructive states. Motion is short and functional.

## Test And Acceptance Criteria

- Scanner ignores junctions, partial directories, copy-suffixed folders, empty folders, and directories without a valid entrypoint.
- `26.623.13972.0` resolves to Codex; `26.707.3748.0` resolves to ChatGPT.
- Default launch selects newest valid version; explicit launch selects the requested valid version.
- Cross-version running conflicts require confirmation and then switch cleanly.
- Retention keeps five by default, supports keep-all, and never removes a running version.
- Delete guards protect running and final versions and repair `current` plus config after deletion.
- Frontend tests cover navigation, dynamic product naming, version actions, retention modes, switch confirmation, and update/switch states.
- Required release checks and `scripts/package-release.ps1` pass; desktop screenshots show no clipping or overlap at minimum and default window sizes.

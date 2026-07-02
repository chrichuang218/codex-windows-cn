# codex-windows-updater

NOTICE: recent Codex updates caused the Electron Codex app to mess with Start Menu shortcuts, and some other stuff that breaks auto updates. In addition to introdoucing some bugs. I will fix it when I have time.

Unofficial Windows installer and updater for the **OpenAI Codex** desktop app.
Downloads the official Microsoft Store MSIX directly (or via `winget`),
extracts it, and re-launches the newest version on demand. No Store app
required.

<img width="609" height="475" alt="screenshot" src="https://github.com/user-attachments/assets/f4b36a50-60ed-4136-8535-86302d9d6fd6" />

## TL;DR

A single executable, `codex-launcher.exe`. It behaves as either:

- **Installer** — when no `updater.json` sits next to it. Runs the wizard
  (mode → path → options → progress → done).
- **Proxy** — when `updater.json` is present. Spawns the most recent
  `Codex.exe` from `versions/<ver>/`, after an optional update check.

Install layout:

```
<root>/
├── codex-launcher.exe       # the same exe, copied here at install time
├── updater.json             # configuration + state
├── versions/
│   ├── 26.422.2437.0/       # extracted MSIX contents
│   ├── 26.500.0.0/
│   └── current → 26.500.0.0 # optional directory junction
└── downloads/               # MSIX cache (one file per update)
```

Not affiliated with OpenAI or Microsoft. The Codex app, its branding, and
its packaging belong to OpenAI.

## Download

Latest release:
[**codex-launcher.exe**](https://github.com/vaportail/codex-windows-updater/releases/latest/download/codex-launcher.exe)
([SHA-256](https://github.com/vaportail/codex-windows-updater/releases/latest/download/codex-launcher.exe.sha256))

Run the binary — it'll launch the installer wizard.

> Windows SmartScreen will warn on first run because the binary isn't
> signed (see [License](#license)). Click "More info" → "Run anyway."

### Verifying the build

Each release is built by GitHub Actions and signed via Sigstore
build-provenance. To verify the binary you downloaded was produced by
this exact repo at the tagged commit (requires
[GitHub CLI](https://cli.github.com/)):

```
gh attestation verify codex-launcher.exe --owner vaportail
```

For a basic integrity check without `gh`, compare the SHA-256:

```powershell
(Get-FileHash codex-launcher.exe -Algorithm SHA256).Hash
# compare against the contents of codex-launcher.exe.sha256
```

## Building from source

Requires Rust 1.80+ and the MSVC toolchain on Windows.

```
cargo build --release
```

The output is `target/release/codex-launcher.exe`.

## Operation

### Install modes

| Mode | Default location | Admin? | Shortcut | Registry |
|---|---|---|---|---|
| Portable | `<cwd>/CodexPortable` | no | optional | no |
| User | `%LOCALAPPDATA%/Codex` | no | optional | HKCU |
| System | `C:\Program Files\Codex` | yes (UAC) | optional | HKLM |

Choose during the wizard. The wizard re-spawns elevated automatically when
System mode is selected.

### Update fetchers

Two strategies for resolving and downloading the latest MSIX:

- **Direct** (default) — DisplayCatalog + FE3 SOAP, anonymous. No local
  tools needed. Pure HTTPS to `displaycatalog.mp.microsoft.com` and
  `fe3.delivery.mp.microsoft.com`.
- **Winget** — shells out to `winget.exe download`. Useful as a manual
  fallback if the direct path is blocked (corporate firewall, custom root
  CA policy, etc.).

If the configured fetcher fails, the launcher transparently retries with
the other one for that run. Your stored preference is **not** auto-flipped
on a single fallback success — a transient blip shouldn't permanently
demote your choice.

A third manual escape hatch exists: pre-download the MSIX yourself and
pass `--msix <path>` along with `--fetcher local`.

### Update checks

`updater.json` carries an `update_policy`: `always` / `daily` / `weekly` /
`never`. Proxy mode honors the policy on each launch:

- Cooldown not elapsed → silent launch, no network.
- Cooldown elapsed → resolve latest version (no download). On match,
  silent launch. On mismatch, show the prompt (Update now / Not now /
  Skip this version / Snooze 1 day / Snooze 7 days / Never).
- "Skip this version" suppresses prompts only while the Store's latest
  matches that version. As soon as Microsoft publishes a newer build,
  prompts resume.

Click "Check for updates" anywhere to bypass the cooldown.

### Versions and pruning

Each MSIX extracts into its own `versions/<ver>/` directory. The launcher
keeps the N newest (default 2) and prunes the rest after a successful
install.

If the optional `versions/current` junction is enabled, it always points
at the newest installed version — useful for tooling, AV exemptions, or
shortcuts that want a stable path.

### Uninstall

Run `codex-launcher.exe --uninstall`. The flow:

1. Validate the install root looks like ours (refuses to wipe a Desktop /
   Downloads / user-profile / Program Files / drive-root).
2. Detect any running `Codex.exe` processes and prompt before terminating
   them.
3. Whitelist-delete only the things we placed: `versions/`, `downloads/`,
   `updater.json`, the Start Menu shortcut, the Add/Remove Programs
   registry entry, the `versions/current` junction.
4. POSIX-unlink the launcher itself (Win10 1809+); falls back to
   `MoveFileEx(MOVEFILE_DELAY_UNTIL_REBOOT)` on older systems.
5. Write a cleanup report to `%TEMP%/codex-uninstall-<ts>.log`.

Files in the install root that we didn't put there are **not** touched.

### Configuration

`updater.json` example:

```json
{
  "install_mode": "user",
  "current_version": "26.500.0.0",
  "update_policy": "daily",
  "last_check_unix": 1735689600,
  "suppress_until_unix": null,
  "known_latest": "26.500.0.0",
  "skipped_version": null,
  "keep_versions": 2,
  "fetcher": "direct",
  "use_current_junction": true
}
```

Edit by hand at your own risk; the launcher writes to it on every
successful update.

### CLI flags

| Flag | Effect |
|---|---|
| `--uninstall` | Run the uninstaller UI |
| `--fetcher direct\|winget\|local` | Override `fetcher` for this run only |
| `--msix <path>` | Use a local MSIX (with `--fetcher local`) |
| `--test-fetch` | Resolve latest version, no download |
| `--test-download` | Download latest MSIX into `downloads/` |
| `--test-extract --version X --root Y` | Extract a downloaded MSIX |
| `--dump-sync` | Dump the raw FE3 SyncUpdates response (debug) |

## License

This project's source code is licensed under the **MIT License** (see
`LICENSE`). The MIT terms cover the code in this repository.

The shipped binary additionally includes third-party components with
their own license terms:

- **[Slint](https://slint.dev/)** — the UI toolkit. Used here under the
  **Slint Royalty-Free License 2.0**, which requires that the running
  application display "Made with Slint" in a place visible to the user
  during normal use. This attribution appears as a small footer on every
  screen of the launcher. See https://github.com/slint-ui/slint for the
  full license text.
- Other dependencies (`reqwest`, `rustls`, `serde`, `windows`, `zip`,
  `quick-xml`, `directories`, `anyhow`, `thiserror`, `winresource`, and
  their transitive deps) are licensed under permissive terms (MIT,
  Apache-2.0, ISC, or combinations thereof). Their license texts are
  preserved in the published crates and follow you wherever the binary
  goes.

If you redistribute the compiled launcher, comply with all of the above —
chiefly: keep the "Made with Slint" attribution visible, and preserve
upstream license notices.

---

Most of this code was written with Anthropic's Claude. Yes, the irony of
shipping a Codex launcher built by a competing AI is fully appreciated.

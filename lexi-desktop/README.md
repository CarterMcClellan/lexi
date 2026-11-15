# lexi-desktop

The Lexi desktop app — a Tauri **thin webview shell over `lexi-host`** (which is
itself a thin shell over the pure `lexi-core` engine). No Node, no Python, no
localhost server. It reuses the original web frontend (a snapshot of the former
Node GUI's single-page UI), with a `fetch`-shim that routes its `/api/*` calls to
in-process Tauri commands
(`src/backend.rs`). It reads the same `~/.lexi-cli/` config as the CLI.

## Run / build

```sh
cargo install tauri-cli --version '^2'   # once
cd lexi-desktop

cargo tauri dev        # run with devtools
cargo tauri build      # produce native installers (see target/release/bundle/)
```

`cargo tauri icon icons/source.png` regenerates the icon set from `source.png`.

A native build is tiny (~4 MB `.dmg` / ~13 MB `.app` on macOS) — the point of the
Rust-core architecture vs. bundling Chromium + Node + Python.

## What works

- Chat (buffered **and** streaming), multi-provider, over `lexi-host`. Providers
  are fanned out **concurrently** — the slowest provider doesn't block the others.
- **Every endpoint the frontend speaks is wired** — chat (buffered + streaming)
  plus all admin tabs: config, prompts, models (incl. hosted-model discovery via
  a live provider call), and API-definition management (create / edit-save /
  delete / validate / live preview). All read and write `~/.lexi-cli/` over
  `lexi-host`; definition validation, flag/preset introspection, and preview
  compose reuse `lexi-core`.

## Releases

`.github/workflows/release.yml` builds installers for macOS (universal), Linux
(`.deb` / `.AppImage`), and Windows (`.msi` / NSIS `.exe`) on a `v*` tag via
`tauri-action`, attaching them to a draft GitHub Release.

```sh
git tag v0.1.0 && git push origin v0.1.0
```

## Signing / notarization (needs your credentials)

The release workflow is **already wired for signing** — it stays unsigned (and
green) until the secrets below exist, then signs automatically on the next tag.
Unsigned builds warn on first launch (macOS Gatekeeper, Windows SmartScreen).

Add these repo secrets (**Settings → Secrets and variables → Actions**):

- **macOS** (Developer ID + notarization; `tauri-action` picks them up):
  - `APPLE_CERTIFICATE` — base64 of your Developer ID `.p12` (`base64 -i cert.p12 | pbcopy`)
  - `APPLE_CERTIFICATE_PASSWORD` — the `.p12` password
  - `APPLE_SIGNING_IDENTITY` — e.g. `Developer ID Application: You (TEAMID)`
  - `APPLE_ID` — your Apple ID email
  - `APPLE_PASSWORD` — an app-specific password for that Apple ID
  - `APPLE_TEAM_ID` — your 10-char Developer Team ID
- **Windows** (Authenticode): a `.pfx` code-signing cert — the workflow imports
  it and sets `tauri.conf.json`'s `certificateThumbprint`:
  - `WINDOWS_CERTIFICATE` — base64 of the `.pfx`
  - `WINDOWS_CERTIFICATE_PASSWORD` — the `.pfx` password
- **Linux**: typically unsigned; `.AppImage` runs as-is.

Once the secrets are in place, re-run the release (`git tag v0.1.1 && git push
origin v0.1.1`) to get signed artifacts.

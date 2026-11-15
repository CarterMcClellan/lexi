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

- Chat (buffered **and** streaming), multi-provider, over `lexi-host`.
- The admin **read** endpoints are real, so the management tabs display live data:
  models (`/api/models`), API definitions (`/api/definitions` + `/raw`/`{name}`),
  prompts (`/api/prompts`), and config (`/api/config`) — all read from `~/.lexi-cli/`.
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

## Signing / notarization (follow-up — needs your credentials)

The release builds are **unsigned**, so the OS warns on first launch
(macOS Gatekeeper, Windows SmartScreen). To ship signed/notarized builds:

- **macOS**: an Apple Developer ID certificate + notarization. Provide
  `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`,
  `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` as repo secrets; `tauri-action`
  picks them up.
- **Windows**: an Authenticode code-signing certificate.
- **Linux**: typically unsigned; `.AppImage` runs as-is.

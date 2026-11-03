# Manual GitHub builds for Mac (with Apple Intelligence) and Windows

Goal: release 0.11.0 for Mac (Apple Silicon, with Apple Intelligence) and Windows, built on GitHub
only when Grizzly asks. Pushing to main must never start a build.

## Decisions (approved by Grizzly 2026-11-03: "Sí usemos GitHub sin que sea automático")

1. GitHub Actions back on, but every workflow is `workflow_dispatch` only (no `push` / `pull_request`
   triggers). Actions has been off since 2026-11-02 to save minutes.
2. Builds are unsigned: no Apple certificate and no Azure Trusted Signing (those were Handy's, cjpais).
   Mac is ad-hoc signed as today. On Windows SmartScreen shows "Windows protected your PC" →
   "More info" → "Run anyway".
3. The Claude API key and the MCP tokens on Windows go in Windows Credential Manager (`keyring`
   `windows-native`). Today Windows would keep the MCP tokens only in memory and cannot save the Claude key.
4. Windows engines: Claude and Ollama. Apple Intelligence is Mac only. Ollama auto-start stays Mac only.
5. The release workflow creates a **draft**. Grizzly publishes it (end gate).
6. Platforms in this release: macOS arm64 (macos-26), Windows x64. Intel Mac, Windows ARM and Linux
   are left out of the matrix to save time (can be added back).

## Files

- `.github/workflows/*.yml` — triggers → `workflow_dispatch`; release matrix trimmed; signing steps
  skipped when the secrets are missing.
- `src-tauri/tauri.conf.json` — remove Handy's Windows `signCommand`.
- `src-tauri/Cargo.toml` — `keyring` features `apple-native`, `windows-native`.
- `src-tauri/src/transform.rs` (`key_store`), `src-tauri/src/voice_source/secrets.rs` (`backend`) —
  real store on macOS and Windows.

## Checks

- `cargo test --lib transform::` and `voice_source::secrets` on this Mac.
- Manual Release run on GitHub → draft v0.11.0 has a Mac .zip/.dmg and a Windows installer.
- Mac build from GitHub: Apple Intelligence reports available on this Mac.

## Pending

- [x] Actions minutes: standard runners (incl. macos-26, windows-latest) are free and unlimited on public repos (docs.github.com, actions billing). Repo is PUBLIC.
- [ ] Workflows manual-only, Actions on.
- [ ] Windows key storage.
- [ ] Release run → draft.
- [ ] Grizzly: test on a Windows PC, then publish the draft.

## Decision 2026-11-03 (Grizzly: "Sí")

- Transform selection (rewrite) is macOS Apple Silicon only (`transform.rs` `platform` module, Cmd+C copy).
  Windows 0.11.0 ships with dictation only; release notes say rewrite on Windows is coming. Porting it
  (Ctrl+C copy, Windows clipboard snapshot) is a separate piece, tested on a real Windows PC.
- Grizzly approved publishing 0.11.0 with both downloads once the draft has the Mac and Windows files.

Release run: https://github.com/grizzlytec/talkngo-mac/actions/runs/41734735750

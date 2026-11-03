# Claude as a third Transform engine

Goal: "Transform selection" (rewrite selected text) can run on Claude over the internet, next to the
existing Ollama (local) and Apple Intelligence (on-device) engines. Each user picks. Nothing is removed.

## Why (2026-11-03)

- Grizzly does not want Ollama running all the time, and found Ollama slow and poor before.
- Apple Intelligence test on this Mac (12 Spanish rewrites, same prompts as the app, plain `respond`
  because Command Line Tools lack the `@Generable` macro): always Spanish, ~4 s for "My voice", but
  1/3 friend-text runs answered "sí" instead of rewriting, another changed meaning; grammar mode left
  accents unfixed ("revise", "numeros", "estan"). Formal was fine.
- The app already has a native Anthropic client (`src-tauri/src/llm_client.rs`, `send_anthropic_message`)
  used by dictation post-processing; Transform does not use it yet.

## Decisions (approved by Grizzly 2026-11-03: "adelante")

1. New `TransformEngine::Claude`, label "Claude (internet)". Ollama and Apple Intelligence stay as they
   are; default engine stays Ollama for new installs.
2. API key entered once in Settings → Transform, stored in the macOS keychain (`keyring` crate, as the
   MCP tokens are), shown masked (last 4 chars) with a "Remove" button. Not written to the settings file.
3. Model picker: Claude Haiku 5.5 (`claude-haiku-5-5`, default) or Claude Sonnet 5.5 (`claude-sonnet-5-5`).
   Cost hint in the UI: Haiku ≈ $0.30 / 1,000 "My voice" rewrites, Sonnet ≈ $6 / 1,000
   (≈2.5k input + 150 output tokens; Haiku $0.10/$0.50, Sonnet $2/$10 per MTok).
4. Same system prompts as the other engines (`build_system_prompt`). Effort `low`; no other tuning.
5. Failure (no internet, key rejected, timeout 15 s): the existing failure path — selection untouched,
   clipboard restored, overlay shows a short error. Settings shows "Key rejected" when Anthropic returns 401.
6. Privacy line under the engine: "The text you select is sent to Anthropic to rewrite it."
7. Release as 0.11.0 (zip + GitHub release as in 0.10.0), installed on this Mac with engine = Claude Haiku.

Mockups: `docs/mockups/2026-11-03-claude-engine.html` (1 first time, 2 ready, 3 errors).

## Not in this plan (proposals)

- Ollama "Install" button and auto-quit after idle — separate piece if wanted.

## Files

- `src-tauri/src/transform.rs` — `TransformEngine::Claude`, model setting, dispatch to `llm_client`.
- `src-tauri/src/llm_client.rs` — reuse `send_anthropic_message` (system prompt + effort if needed).
- `src-tauri/src/settings.rs` — `transform_claude_model`.
- New key commands (save / remove / masked status) in `transform.rs` via `keyring`.
- `src/components/settings/transform/TransformSettings.tsx` — third engine button, key row, model select.
- `src/i18n/locales/*/translation.json` — `settings.transform.engine.claude*` (en + es, English copied to others).

## Checks

- `cargo test` for the new transform/llm_client tests only; `bun run check:translations && bun run lint`.
- Screenshot of the three states vs the mockup.
- End-to-end on this Mac: the 12 Spanish cases through Haiku (key from `.env.local`, run via
  `~/.claude/bin/with-env`), then a real rewrite in the installed app.

## Pending

- [x] Grizzly approves decisions + mockups ("adelante").
- [x] Grizzly pasted `ANTHROPIC_API_KEY` into `.env.local` (test only; users enter their key in Settings → Transform, never a file — Grizzly, 2026-11-03).
- [x] Haiku 5.5 test, same 12 Spanish cases, effort low: 12/12 Spanish, 0.7–1.9 s; "My voice" kept the question as a question and all content; friend text rewritten (no "sí" answers); grammar fixed accents (revisé, números, están); formal fine. 22,354 in / 604 out tokens ≈ $0.0025 total.

## Progress (2026-11-03)

- Implemented (backend + UI + i18n). Added: `effort: low` on Claude rewrite requests (verified accepted by
  Haiku 5.5 in the test above); overlay failure notice for Claude ("No internet / Check your API key /
  Couldn't rewrite — text left as it was", 2.5 s) — local engines keep their silent failure.
- Independent review: no blocking issues; 3 minor items fixed (bindings doc comment, Remove error shown,
  error body truncated to 200 chars).
- Checks: `cargo test --lib transform::` 30 passed; `cargo test --lib llm_client::` 22 passed;
  `bun run check:translations`, `bun run lint`, `bunx tsc --noEmit`, prettier, `cargo fmt --check` pass.
  Clippy warnings only in untouched `managers/model.rs`.
- Version 0.11.0. Build on this Mac (Command Line Tools only, no Xcode) compiles Apple Intelligence as a
  stub → this build is for Grizzly's Mac only; the public GitHub release must be built on a Mac with full
  Xcode so Apple Intelligence keeps working for other users.
- Screenshots vs mockup: `docs/screenshots/2026-11-03-claude-engine-{1,2,3}.png` match (states, copy, key
  masking). Fixed: engine status no longer wraps onto two lines. Engine picker keeps the app's existing
  button style instead of the mockup's radio cards.
- Installed 0.11.0 on Grizzly's Mac (0.10.0 backup in `~/TalknGo-old-backup/`). Accessibility reset with
  `tccutil reset Accessibility com.grizzlytec.talkngo` because the new ad-hoc signature invalidated the grant.

## Still pending

- Grizzly: re-grant Accessibility, save his key in Settings → Transform, one real rewrite.
- Public GitHub release v0.11.0: build on a Mac with full Xcode (this Mac stubs Apple Intelligence).

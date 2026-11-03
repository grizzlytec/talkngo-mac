# Voice Sources Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** In Settings → Transform the person picks where their voice guide and special words come from (their MCP server, a file, or written in the app); Kodiak gets a read-only `voice` access for it.

**Architecture:** Kodiak's MCP server learns a second OAuth scope `voice` (loopback redirects, revocable client rows, only two read tools). TalknGo gets a small OAuth 2.1 + PKCE + DCR client, a minimal Streamable-HTTP MCP client, Keychain token storage, and a source-aware sync that caches guide + words on disk; dictation and rewrite read only the cache.

**Tech Stack:** Kodiak: TypeScript, Express, `@modelcontextprotocol/sdk`, node:test via tsx, Supabase (schema `kodiak`), Coolify deploy on push to main. TalknGo: Rust (Tauri 2, reqwest 0.12, tokio), React + TS, tauri-specta bindings, bun.

**Spec:** `docs/superpowers/specs/2026-11-03-voice-sources-design.md` (approved 2026-11-03). Mockups: `docs/mockups/2026-11-03-voice-sources.html`.

**Repos:** TalknGo = `/Volumes/Internal/Software Development/anvilagents/talkngo-mac` (branch main). Kodiak = `/Volumes/Internal/Software Development/anvilagents/KODIAK` (`git pull --rebase` first; working copy was stale). Research notes: scratchpad `kodiak-facts.md`, `talkngo-facts.md` (copied facts are inlined below where they decide something).

## Global Constraints

- No Kodiak-specific code, name or URL in TalknGo. The MCP option label is "Your own MCP connection"; after connecting show the server's own `serverInfo.name`.
- A `voice` token can list/call only `get_voice_guide` and `get_vocabulary`. Full `mcp` tokens and claude.ai behavior unchanged.
- Dictation/rewrite never wait on the network: they read cached files only.
- Sync: at app start, every 3600 s, and on "Sync now". Offline/failure keeps the last cache.
- Vocabulary from a source is merged with `custom_words`: case-insensitive de-dupe, app words win on spelling.
- Tokens only in the macOS Keychain (`cfg(target_os = "macos")`); CI runs `cargo test` on Ubuntu, so non-macOS builds must compile (in-memory fallback).
- Legacy `voice_sync.json` keeps working silently while `voice_source` is unset.
- The built-in `voice_guide.md` (Write it here) is never overwritten by an MCP sync.
- i18n: every new key in all 24 locales (`bun run check:translations` fails on missing or extra); English text, Spanish translated in `es`.
- Kodiak: every migration creating a table grants explicitly (RLS on; revoke all from public/anon/authenticated/service_role; grant needed privileges to `service_role` only). Apply with Supabase MCP `apply_migration` (project `feqmffwvfcprblrairuc`). Run `rls-reviewer` on it.
- Kodiak production changes (migration, deploy, Coolify env) are approved with the spec. Undo for each: migration → `drop table`; code → `git revert` + push (Coolify redeploys); no env var change is needed (loopback rule is code).
- Commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Push to main, `git pull --rebase` first, never `--no-verify`.

## Review Focus

1. Token expired or revoked mid-use → sync shows "Sign in again", the cached guide/words keep working, no retry storm (one refresh attempt per sync).
2. MCP server answers `tools/call` as `text/event-stream` instead of JSON → still parsed (both content types supported).
3. Person closes the browser without consenting → Connect gives up after 300 s, closes the loopback listener, UI back to "not connected".
4. Tool returns empty text, an error result, or more than 200 KB → previous cache kept, status shows the error.
5. Server address pasted with trailing slash / without `/mcp` path / with spaces → trimmed; used exactly as given otherwise; discovery uses the URL's origin.

---

## Part A — Kodiak

### Task A1: Storage for voice clients and vocabulary

**Files:**
- Create: `supabase/migrations/20261103120000_voice_clients_and_vocabulary.sql`

Tables in schema `kodiak`:
- `voice_clients(id uuid pk default gen_random_uuid(), label text not null, created_at timestamptz not null default now(), last_used_at timestamptz, revoked_at timestamptz)`
- `dictation_vocabulary(id smallint pk default 1 check (id = 1), words text[] not null default '{}', updated_at timestamptz not null default now())` + insert the single row seeded from `TalknGo/vocabulary.txt` (one entry per non-empty line, in `/Volumes/Internal/Software Development/anvilagents/TalknGo/vocabulary.txt`).
- Both: `enable row level security`; `revoke all ... from public, anon, authenticated, service_role`; `grant select, insert, update on kodiak.voice_clients to service_role`; `grant select, update on kodiak.dictation_vocabulary to service_role`. No policies (service role bypasses RLS; nobody else gets access).

- [ ] Step 1: Write the migration file (pattern: `supabase/migrations/0124_voice_guide_versions.sql` lines 14-28).
- [ ] Step 2: Apply with Supabase MCP `apply_migration` (name `voice_clients_and_vocabulary`). Verify: `execute_sql` `select count(*) from kodiak.dictation_vocabulary` → 1; `select array_length(words,1) ...` → line count of vocabulary.txt.
- [ ] Step 3: Run `rls-reviewer` agent on the migration; fix findings.
- [ ] Step 4: Commit `feat(db): voice_clients and dictation_vocabulary for TalknGo voice access`.

### Task A2: OAuth `voice` scope, loopback redirects, client rows

**Files:**
- Modify: `mcp/src/oauth.ts` (metadata :35-47, authorizeGet :155-209, authorizePost :211-266, /token :268-330, refresh :339-370), `mcp/src/tokens.ts` (payload types :65-85), `mcp/src/index.ts` (CSP form-action :56-64)
- Create: `mcp/src/voice/clients.ts`
- Test: `mcp/src/__tests__/oauth-voice-scope.test.ts`

**Interfaces:**
- Produces: `type OAuthScope = 'mcp' | 'voice'`; `AccessTokenPayload` / `RefreshTokenPayload` gain optional `vcid?: string` (voice client id). `voice/clients.ts`: `createVoiceClient(label: string): Promise<string>`, `isVoiceClientActive(id: string): Promise<boolean>` (60 s in-memory cache; also bumps `last_used_at` at most once per hour), `listVoiceClients(): Promise<VoiceClient[]>`, `revokeVoiceClient(id: string): Promise<boolean>`, `__resetVoiceClientCacheForTests()`.

Rules:
- `scopes_supported: ['mcp', 'voice']`. Requested scope: absent → `mcp`; contains `voice` and nothing else → `voice`; anything else → 400 `invalid_scope`.
- Redirect check: scope `mcp` → existing exact allowlist. Scope `voice` → must match `^http://127\.0\.0\.1:\d{1,5}/callback$` (also allowed: exact allowlist). Loopback is never accepted for `mcp`.
- `scope` travels as a hidden input from authorizeGet to authorizePost and into the code payload (`AuthCodePayload.scope`). The consent page for `voice` says "TalknGo (or another app) wants read-only access to your voice guide and dictation words." Owner passphrase still required.
- /token with a `voice` code: `createVoiceClient(label)` where label = `client_name` sent at /register if echoed into the code, else `"Voice app"` plus the date; access + refresh carry `scope:'voice'`, `vcid`. Response `scope: 'voice'`. Refresh grant copies `vcid` and refuses if `!isVoiceClientActive(vcid)` (400 `invalid_grant`).
- `/register` stays stateless but echoes `client_name` and `scope`; authorizeGet accepts optional `client_name` query param (TalknGo sends it) and carries it to the code.
- CSP `form-action`: add `http://127.0.0.1:*`.

- [ ] Step 1: Failing tests (style of `oauth-redirect-allowlist.test.ts`, fake req/res, `import './env-setup.js'` first, monkeypatch `supabase.schema`):
  - `voice scope accepts loopback redirect` (authorizeGet with scope=voice, redirect `http://127.0.0.1:53682/callback` → 200 consent page containing "read-only")
  - `mcp scope rejects loopback redirect` → 400
  - `unknown scope rejected` (scope=`mcp voice`) → 400 `invalid_scope`
  - `voice code yields voice tokens with vcid` (authorizePost with correct passphrase → code; /token → body.scope === 'voice'; verifyAccessToken(access).vcid is the id returned by a stubbed createVoiceClient)
  - `refresh refused after revoke` (stub isVoiceClientActive → false → 400 invalid_grant)
  - `mcp flow unchanged` (existing tests still pass)
- [ ] Step 2: `cd mcp && npm test` → new tests FAIL.
- [ ] Step 3: Implement.
- [ ] Step 4: `cd mcp && npm run typecheck && npm test` → all PASS.
- [ ] Step 5: Commit `feat(mcp): read-only voice OAuth scope with loopback redirects and revocable clients`.

### Task A3: Voice tools and scope-filtered MCP server

**Files:**
- Modify: `mcp/src/voice/verbs.ts`, `mcp/src/verbs-registry.ts` (VERBS :240, voice block :358), `mcp/src/mcp-server.ts` (`buildMcpServer` :33, loop :61-64), `mcp/src/index.ts` (POST /mcp :111-132), `mcp/src/auth.ts` (`requireBearer` :17-40)
- Test: `mcp/src/__tests__/voice-scope-tools.test.ts`

**Interfaces:**
- Consumes: `isVoiceClientActive`, `listVoiceClients`, `revokeVoiceClient` (A2); `loadGuide(): Promise<{version:number; body:string; token:string}>` (`voice/guide.ts:148`).
- Produces: `buildMcpServer(opts?: { scope?: 'mcp' | 'voice' })`; `VOICE_SCOPE_VERBS: ReadonlySet<string> = new Set(['get_voice_guide','get_vocabulary'])`.

Verbs:
- `get_voice_guide` — no args; returns JSON text `{"version": <int>, "guide": "<body>"}`. Read-only.
- `get_vocabulary` — no args; returns JSON text `{"words": [...]}` from `kodiak.dictation_vocabulary`.
- `set_vocabulary` — `{ words: string[] }` (max 2000 items, each 1-100 chars, trimmed, de-duped case-insensitively) — full scope only.
- `voice_clients` — lists `{id,label,created_at,last_used_at,revoked_at}` — full scope only.
- `revoke_voice_client` — `{ id: uuid }` — full scope only; returns whether a row was revoked.

Rules: POST /mcp passes `req.auth.scope` to `buildMcpServer`; with `voice` only `VOICE_SCOPE_VERBS` are registered and handlers get ctx scope `'voice'`. `requireBearer`: a token with `scope:'voice'` must have `vcid` and `isVoiceClientActive(vcid)` → else 401 with `WWW-Authenticate: Bearer error="invalid_token"`. Check `mcp-tool-parity.ts` and `schema_info` don't expose other verbs to a voice token (adjust if they do).

- [ ] Step 1: Failing tests: `voice server lists only two tools` (buildMcpServer({scope:'voice'}) → tools/list names deep-equal `['get_vocabulary','get_voice_guide']` sorted); `full server includes the new verbs`; `get_voice_guide returns version and guide` (stub rpc voice_guide_current → `{version:4, body:'Be brief.'}` → parsed `{version:4, guide:'Be brief.'}`); `set_vocabulary dedupes case-insensitively` (`['Loxone','loxone',' Touch Pure ']` → stored `['Loxone','Touch Pure']`); `revoked voice token gets 401` (requireBearer with stub inactive).
- [ ] Step 2: `cd mcp && npm test` → FAIL.
- [ ] Step 3: Implement.
- [ ] Step 4: `cd mcp && npm run typecheck && npm test` → PASS.
- [ ] Step 5: Commit `feat(mcp): get_voice_guide/get_vocabulary for voice tokens; vocabulary and voice-client admin verbs`.

### Task A4: Deploy Kodiak and check live

- [ ] Step 1: `git pull --rebase && git push origin main` (pre-push hook runs typecheck + tests; Coolify auto-deploys `mcp/**`). Do not call the Coolify deploy API.
- [ ] Step 2: Wait (background) until `https://kodiak.grizzlytec.com/health` is OK and `/.well-known/oauth-authorization-server` lists `"voice"` in `scopes_supported`. Use `ctx_execute` fetch (curl is blocked).
- [ ] Step 3: Check claude.ai's Kodiak connector still works: call `mcp__claude_ai_Kodiak__ping` → OK.
- [ ] Undo if broken: `git revert <shas> && git push`; tables are unused by the old code so they can stay.

## Part B — TalknGo

### Task B1: Source settings

**Files:**
- Modify: `src-tauri/src/settings.rs` (AppSettings ~:563-576, `get_default_settings` ~:1090), `src-tauri/src/transform.rs` (commands next to :213), `src-tauri/src/lib.rs` (collect_commands :740-747), `src/stores/settingsStore.ts` (settingUpdaters :183-191)

**Interfaces:**
- Produces: `enum VoiceSource { BuiltIn, File, Mcp }` (derives like `TransformEngine` at transform.rs:40, `#[serde(rename_all="snake_case")]`) in `transform.rs`. AppSettings fields, all `#[serde(default)]`: `voice_source: Option<VoiceSource>` (None = legacy behavior), `voice_mcp_url: Option<String>`, `voice_mcp_guide_tool: Option<String>`, `voice_mcp_words_tool: Option<String>`, `voice_words_path: Option<String>`. Commands: `change_voice_source_setting(app, source: VoiceSource)`, `change_voice_words_path_setting(app, path: Option<String>)` (same extension check as the guide path), `change_voice_mcp_tools_setting(app, guide: Option<String>, words: Option<String>)`.
- `fn effective_voice_source(s: &AppSettings) -> VoiceSource`: `Some(x)` → x; None → `File` if `voice_guide_path` is set, else `BuiltIn`.

- [ ] Step 1: Failing tests in settings tests: `stores_without_voice_source_load_as_none`; `effective_source_is_file_when_legacy_path_set`; `effective_source_defaults_to_built_in`.
- [ ] Step 2: `cd src-tauri && cargo test --lib settings` → FAIL. Step 3: implement. Step 4: PASS (and `cargo build` once to regenerate `src/bindings.ts`).
- [ ] Step 5: Commit `feat: voice source settings`.

### Task B2: Keychain token store

**Files:**
- Create: `src-tauri/src/voice_source/secrets.rs`; add `keyring = { version = "3", features = ["apple-native"] }` under `[target.'cfg(target_os = "macos")'.dependencies]`.

**Interfaces:**
- Produces: `struct StoredTokens { access_token: String, refresh_token: Option<String>, expires_at: Option<i64>, client_id: String, token_endpoint: String }` (serde); `fn save_tokens(server_url: &str, t: &StoredTokens) -> Result<(), String>`, `fn load_tokens(server_url: &str) -> Result<Option<StoredTokens>, String>`, `fn delete_tokens(server_url: &str) -> Result<(), String>`. Keychain service `"TalknGo MCP"`, account = server URL. Non-macOS and `cfg(test)`: process-wide in-memory map.
- [ ] Steps: test `save_load_delete_roundtrip` (in-memory backend) → fail → implement → pass → commit `feat: keychain storage for MCP tokens`.

### Task B3: OAuth client (discovery, DCR, PKCE, loopback)

**Files:**
- Create: `src-tauri/src/voice_source/oauth.rs`; deps: `rand = "0.8"`, `base64 = "0.22"`, `url = "2"` (tokio already present; add features `net`, `io-util`, `time`, `macros` to the main tokio dep).

**Interfaces:**
- Consumes: B2 `save_tokens`.
- Produces:
  - `struct AuthServer { authorization_endpoint: String, token_endpoint: String, registration_endpoint: Option<String>, scopes_supported: Vec<String> }`
  - `async fn discover(server_url: &str) -> Result<AuthServer, String>` — GET `<origin>/.well-known/oauth-protected-resource` → first `authorization_servers[0]`; fallback `<origin>/.well-known/oauth-authorization-server`.
  - `fn pkce_pair() -> (String /*verifier, 64 chars*/, String /*S256 challenge b64url no pad*/)`
  - `fn authorize_url(as_: &AuthServer, client_id: &str, redirect: &str, challenge: &str, state: &str, scope: Option<&str>) -> String` — scope `voice` only when `scopes_supported` contains it; adds `client_name=TalknGo`.
  - `fn parse_callback(request_line: &str, expected_state: &str) -> Result<String /*code*/, String>` (rejects wrong state, `error=` param).
  - `async fn connect(app: &AppHandle, server_url: &str) -> Result<StoredTokens, String>` — bind `127.0.0.1:0`, redirect `http://127.0.0.1:<port>/callback`, DCR (POST JSON `{client_name:"TalknGo", redirect_uris:[redirect], token_endpoint_auth_method:"none", grant_types:["authorization_code","refresh_token"], response_types:["code"], scope}`; if no registration endpoint, error "This server doesn't support app sign-in"), open browser (`OpenerExt::open_url`), wait ≤300 s for one request, reply with a small "You can close this tab" HTML page, exchange code (form POST, PKCE verifier), save tokens.
  - `async fn refresh(t: &StoredTokens) -> Result<StoredTokens, String>`.
- [ ] Step 1: Failing tests: `pkce_challenge_matches_rfc7636_example` (verifier `dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk` → challenge `E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM`); `authorize_url_requests_voice_only_when_supported`; `parse_callback_rejects_wrong_state`; `parse_callback_returns_code`; `discover_falls_back_to_origin_metadata` (local tokio test server).
- [ ] Step 2: FAIL. Step 3: implement. Step 4: `cargo test --lib voice_source` PASS.
- [ ] Step 5: Commit `feat: OAuth sign-in for MCP servers (PKCE, DCR, loopback)`.

### Task B4: Minimal MCP client and tool pick

**Files:**
- Create: `src-tauri/src/voice_source/mcp.rs`

**Interfaces:**
- Consumes: B2, B3 `refresh`.
- Produces: `struct McpTool { name: String, description: String, required_args: usize }`; `struct McpSession { server_name: String, ... }`; `async fn open(url: &str, token: &str) -> Result<McpSession, McpError>` (`initialize` protocolVersion `"2025-06-18"`, clientInfo `{name:"TalknGo", version}`, then `notifications/initialized`; keeps `Mcp-Session-Id` if returned); `async fn list_tools(&self) -> Result<Vec<McpTool>, McpError>`; `async fn call_text(&self, tool: &str) -> Result<String, McpError>` (joins text content; `isError` → Err); `enum McpError { Unauthorized, Other(String) }` (401 → Unauthorized); `fn parse_rpc_body(content_type: &str, body: &str, id: u64) -> Result<serde_json::Value, String>` handles `application/json` and `text/event-stream` (`data:` lines, picks the message with matching id); `fn pick_tools(tools: &[McpTool]) -> (Option<String>, Option<String>)` → `get_voice_guide` / `get_vocabulary` if present; `fn parse_words(text: &str) -> Vec<String>` accepts `{"words":[...]}`, a JSON array, or one word per line.
- [ ] Step 1: Failing tests: `parses_sse_response_by_id`; `parses_json_response`; `pick_tools_finds_convention_names`; `pick_tools_none_when_missing`; `parse_words_accepts_object_array_and_lines`; `call_text_error_result_is_err`.
- [ ] Steps 2-5: FAIL → implement → PASS → commit `feat: minimal MCP client for voice sources`.

### Task B5: Source-aware sync, cache, and word merge

**Files:**
- Create: `src-tauri/src/voice_source/mod.rs` (owns sync), `src-tauri/src/voice_source/cache.rs`
- Modify: `src-tauri/src/voice_sync.rs` (legacy path only runs when `voice_source` is None), `src-tauri/src/transform.rs` (`active_voice_guide_path` :165, `read_voice_guide` :172), `src-tauri/src/managers/transcription.rs` (:1320 and :1643), `src-tauri/src/lib.rs` (:194 start sync; register commands)

**Interfaces:**
- Consumes: B1-B4.
- Produces:
  - cache dir `<app data>/voice_source/`: `guide.md`, `words.json` (`["…"]`), `status.json` = `struct SourceStatus { server_name: Option<String>, guide_version: Option<i64>, synced_at: Option<i64>, error: Option<String>, needs_sign_in: bool }` (serde + specta).
  - `async fn sync_now(app: &AppHandle) -> Result<SourceStatus, String>`: Mcp → load tokens (none → `needs_sign_in`), open, tools from settings or `pick_tools`, `call_text`; on `Unauthorized` refresh once and retry, else `needs_sign_in`; reject empty or > 200 KB guide (keep previous); write files atomically (reuse `write_atomic` pattern voice_sync.rs:54); parse `version` from a JSON guide `{"version","guide"}` if the text is that shape, else plain text. File/BuiltIn → no network, status only.
  - `fn start(app: &AppHandle)` — spawn: sync at start, then every 3600 s; replaces `voice_sync::start_background_sync` call when `voice_source` is Some.
  - `fn guide_path(app) -> PathBuf` by source: BuiltIn → `voice_guide.md`; File → `voice_guide_path`; Mcp → `voice_source/guide.md`; None → legacy (`active_voice_guide_path` today).
  - `fn source_words(app) -> Vec<String>`: Mcp → `words.json`; File → read `voice_words_path` (parse_words) if set; BuiltIn → empty.
  - `fn merge_words(app_words: &[String], source: &[String]) -> Vec<String>` — app words first, then source words not equal (case-insensitive, trimmed) to any already present.
  - transcription.rs: both :1320 and :1643 use `merge_words(&settings.custom_words, &source_words(app))` (compute once per transcription).
  - Commands: `voice_source_connect(app, url: String) -> Result<SourceStatus, String>` (trim URL, save `voice_mcp_url`, `oauth::connect`, `sync_now`), `voice_source_disconnect(app)` (delete tokens + cache, clear url/tools, source → BuiltIn), `voice_source_sync(app) -> Result<SourceStatus,String>`, `voice_source_status(app) -> SourceStatus`, `voice_source_tools(app) -> Result<Vec<McpTool>, String>`.
- [ ] Step 1: Failing tests: `merge_words_app_wins_case_insensitive` (`["Loxone"]` + `["loxone","Touch Pure"]` → `["Loxone","Touch Pure"]`); `guide_path_by_source` (four cases); `oversized_guide_keeps_previous_cache`; `empty_guide_keeps_previous_cache`; `mcp_sync_never_touches_builtin_guide`; `legacy_sync_skipped_when_source_set`.
- [ ] Steps 2-5: FAIL → implement → `cargo test` (all) PASS → commit `feat: voice source sync with cache and word merge`.

### Task B6: Settings UI

**Files:**
- Modify: `src/components/settings/transform/TransformSettings.tsx`, `src/i18n/locales/*/translation.json` (24 locales), `src/stores/settingsStore.ts`

Matches mockup `docs/mockups/2026-11-03-voice-sources.html` (three options; no Google Drive). Group title "My voice guide — source"; radiogroup rows (existing hand-made `role="radio"` + `<Radio checked/>` pattern at :169):
- **MCP server** — sub "Your own MCP connection". Not connected: "Server address" Input (placeholder "Paste your MCP server address") + primary Button "Connect" (spinner + "Waiting for sign-in in your browser…" while connecting) + hint "Your browser opens to sign in. TalknGo keeps the access in your Mac's keychain — nothing to copy between Macs." Connected: address read-only + secondary "Disconnect"; status row with green dot "Connected to <server_name>" and "Voice guide v<version> · updated <relative time>"; secondary "Sync now"; if `needs_sign_in`: warning text "Sign in again" + "Connect" button; if `pick_tools` found nothing, two Selects "Voice guide tool" / "Words tool" (options = zero-required-arg tools + "None") saving via `change_voice_mcp_tools_setting`.
- **File** — sub "A .md or .txt on this computer". "Voice guide file" (path display + "Choose file…") and "Words file (optional)" (same).
- **Write it here** — sub "Built-in guide". Button "Open voice guide" (existing command).
Remove the old standalone sync button/legacy controls from view when `voice_source` is set (keep them when None so legacy users see what they have).

- [ ] Step 1: Implement; add keys under `settings.transform.voiceSource.*` to `en` and `es` (Spanish), copy English to the other 22 locales.
- [ ] Step 2: `bun run check:translations && bun run lint && bun run format:check && bun run build` → all pass.
- [ ] Step 3: Run the app (`bun run tauri dev`), headless screenshot of Settings → Transform in each of the three states; compare with the mockup in the main session; fix differences.
- [ ] Step 4: Commit `feat: voice source picker in Transform settings`.

### Task B7: Docs, end-to-end with Kodiak, release 0.10.0

- [ ] Step 1: README (es + en): replace the "voice guide file" step with the three options, and add "Make your MCP server work with TalknGo": tools `get_voice_guide` (text or `{"version","guide"}`) and `get_vocabulary` (`{"words":[…]}`, JSON array, or lines), OAuth with dynamic client registration and loopback redirects, optional scope `voice`.
- [ ] Step 2: Bump version to `0.10.0` in `package.json`, `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`; `bun run tauri build`; install to /Applications (backup previous to `~/TalknGo-old-backup`); reset TCC for `com.grizzlytec.talkngo` and open Accessibility settings.
- [ ] Step 3: End-to-end (needs Grizzly's Kodiak passphrase in his browser — the only moment he is needed): Settings → Transform → MCP server → paste `https://kodiak.grizzlytec.com/mcp` → Connect → consent page says read-only → status shows "Connected to <name>", guide version 4+, words count > 0. Evidence: `~/Library/Application Support/com.grizzlytec.talkngo/voice_source/status.json` and log lines; Kodiak `voice_clients` verb lists the new client. Then "My voice" rewrite of a selected sentence works. Then revoke via `revoke_voice_client` → next "Sync now" shows "Sign in again" and the cache still works.
- [ ] Step 4: Commit, push, zip (`TalknGo/` with `TalknGo.app` + `LICENSE`, `ditto -c -k --keepParent`), `gh release create v0.10.0` with SHA-256 in notes (es + en).

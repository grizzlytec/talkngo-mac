# Voice sources: connect your MCP, a file, or write it here

Status: APPROVED by Grizzly 2026-11-03 ("ok dejemoslo asi") — includes the Kodiak production changes below
Mockups: `docs/mockups/2026-11-03-voice-sources.html` (approved direction; option label must not say "Kodiak")

## Intent and why

TalknGo's "My voice" rewrite style and its dictation vocabulary need the person's own material: a voice guide
(how they write) and a list of special words. Today the guide comes from a hidden config file
(`voice_sync.json` with a URL and a token) that must be copied by hand to every Mac, or from a local file.
Grizzly works on several Macs and won't remember hidden files; his friends download the public app and
must be able to plug in whatever they use. Everything must be set up **inside the app, the same way on any
Mac, for anyone**.

## Goals

- Settings → Transform gets one "My voice guide — source" picker with three options:
  1. **MCP server** — any MCP server (Grizzly's is Kodiak). Paste the address, click Connect, sign in
     once in the browser.
  2. **File** — a .md/.txt on this Mac (today's "Choose file…", plus pasting a path). A file inside a
     Google Drive/Dropbox synced folder works this way.
  3. **Write it here** — the built-in guide edited in the app (today's default `voice_guide.md`).
- The chosen source provides **two things**: the voice guide and the special-words list (vocabulary).
  Grizzly said "ambas".
- Vocabulary from a source is **added** to the words typed in the app (`custom_words`), never replaces them.
- Sync at app start, every hour, and on "Sync now". The result is cached on disk; offline uses the last copy.
- Dictation and rewrite read only the local cache: **zero added latency per dictation**.
- Access tokens live in the macOS Keychain. No secret files, nothing to copy between Macs.
- On a new Mac the only step is: pick the source, click Connect, sign in.

## Non-goals

- Writing back to the source (TalknGo only reads).
- More than one active source at a time.
- Windows/Linux UI work (the fork builds there, but only macOS is shipped).

## Anti-goals (a failure even if it works)

- Any Kodiak-specific code, name or URL in the public app. The option says "Your own MCP connection";
  only after connecting does it show the name the server itself reports.
- A token on a laptop that can do more than read the voice guide and vocabulary.
- Anything that makes a dictation wait on the network.
- A setup step that needs a terminal, a hidden folder, or a file copied from another Mac.

## Decided by Grizzly (2026-11-03)

- Standardize: no hidden per-Mac files; setup lives in the app ("esto tiene que estar estandarizado").
- Source options as in the mockups; the MCP option must not say "Kodiak".
- **No Google Drive connection** (Grizzly, 2026-11-03: too complicated; people choose where the file is on
  their computer instead). The Drive card is removed from the mockups.
- The connection brings both the voice guide and the vocabulary.
- Kodiak gets a **read-only voice access** for this: a token that can only read the guide and the words.
  Grizzly can disconnect any Mac from Kodiak.

## How it works

### TalknGo — MCP source

- MCP client over Streamable HTTP, OAuth 2.1 authorization-code + PKCE, dynamic client registration
  (what Kodiak and most hosted MCP servers already speak). The browser opens for sign-in; the redirect
  lands on a loopback listener (`http://127.0.0.1:<port>/callback`, RFC 8252). TalknGo requests the scope
  `voice` when the server advertises it, else the server's default.
- Tool discovery (TalknGo convention, documented in the README so anyone can make their server work):
  - guide: a tool named `get_voice_guide` (no required args) returning text/markdown;
  - vocabulary: a tool named `get_vocabulary` returning one word/phrase per line or a JSON string array.
  - If those names aren't found, a menu lists the server's tools with no required arguments so the person
    picks which one gives the guide and which the words (either may be "none").
- Shows "Connected to <server name>" (from the MCP `serverInfo`), guide version if the server returns one,
  and "updated N min ago". Buttons: Sync now, Disconnect (deletes the Keychain entry and the cache).
- Refresh tokens are used silently; if they fail, the status shows "Sign in again" and the cache keeps working.

### TalknGo — File and Write it here

- File: choose or paste a path; read at use time (as today). Optional second file for words.
- Write it here: today's built-in `voice_guide.md`; words stay in the existing custom-words setting.

### Kodiak (separate repo, `grizzlytec/kodiak`, MCP server)

- New OAuth scope `voice`. A `voice` token can list and call only `get_voice_guide` and `get_vocabulary`;
  every other verb is hidden and refused. Existing `mcp` (full) tokens are unchanged.
- `get_voice_guide`: read-only, returns the current guide text and version (same source as `voice_brief`,
  i.e. `kodiak.voice_guide_current()`), no channel/mode arguments.
- `get_vocabulary`: read-only, returns Grizzly's dictation word list. Kodiak has no such list today, so it
  needs a home (see "Left to the builder").
- Redirects: `KODIAK_MCP_REDIRECT_URIS` today allows exact URIs only. Allow loopback redirects
  (`http://127.0.0.1:<any port>/callback`) **only for clients registering with scope `voice`**.
- Disconnecting a Mac: Kodiak today has no server-side token revocation. Add a way to list `voice`
  clients and revoke one (a verb usable from Claude is enough; no UI).
- These are production changes to Kodiak: each one needs its undo written down before it ships, and any
  Supabase change goes through `rls-reviewer` with explicit grants.

### Migration

- If `voice_sync.json` exists, keep honoring it (as today) until the person picks a source in the new
  picker; then it is ignored. The `voice-guide` edge function stays until every Mac of Grizzly's moved;
  removing it is a later cleanup.

## Builder's calls (Claude, 2026-11-03 — Grizzly can override)

- **Kodiak sign-in:** `voice` clients go through the same owner-passphrase consent page as today. So
  "sign in once" on a new Mac = type the Kodiak passphrase once. No lighter consent.
- **`voice` tokens:** only issued after that passphrase consent; refresh tokens rotate; each client is
  listed and revocable (a full-scope verb, e.g. `voice_clients` / `revoke_voice_client`).
- **Vocabulary merge:** source words + app words, de-duplicated case-insensitively, app words win on
  spelling. Used exactly like `custom_words` today (same path to the model).
- **Editing Kodiak's word list:** through Claude with a full-scope verb (`set_vocabulary`); no Kodiak UI.
- **Legacy `voice_sync.json`:** keeps working silently; no migration prompt.

## Constraints

- Public repo, MIT fork of Handy: keep the attribution; no secrets or personal URLs in code.
- Unsigned (ad-hoc) app: every update resets macOS permissions — out of scope here, noted.
- Release as 0.10.0 on GitHub Releases, same zip layout as 0.9.x.

## Left to the builder

- MCP/OAuth crates (e.g. `rmcp`, `oauth2`, `keyring`) and the loopback listener details.
- Exact cache file names and the on-disk format of the cached vocabulary.
- Where Kodiak stores the vocabulary (new table vs. a knowledge entry) and how Grizzly edits it — as long as
  it is read-only through the `voice` scope; seed it from the old TalknGo `vocabulary.txt`.
- UI strings: English source, Spanish translation in `es`.

# Auto language: only the languages the person speaks

## Goal

In Auto, TalknGo must pick only between the person's spoken languages (Grizzly: English and Spanish),
whatever the speech model. Bug seen 2026-11-03: saying "test" with Parakeet Ultra wrote "Тест" (Russian).

## Cause

Parakeet Ultra detects the language itself, so Auto ran the model's own detection over its 25 languages
and ignored `spoken_languages` (`policy.rs` `AutoLanguage::Native`). The existing "Always" detection mode
restricts Auto to spoken languages via the VoxLingua107 detector, but when the clip is under 0.5 s or the
detector is missing/fails it fell back to the model's own detection — the same Russian guess.

## Approved decisions

- 2026-11-03, Grizzly: "inglés y español", not Spanish fixed. Approved changing the code so Auto picks only
  between his languages with any model ("si").

## Change

- `src-tauri/src/language_detection/policy.rs`: in Always mode the detect plan's fallback is
  `FirstLanguage` (first spoken language) instead of `Native`.
- On Grizzly's Mac: turn on Settings → General → language detection "Always" (downloads the detector).
- Default mode stays `WhenNeeded` (Always needs the detector download, which the toggle consents to).

## Test commands

- `cd src-tauri && cargo test --lib language_detection`
- End to end: dictate "test" and a Spanish sentence; log shows `Language detection:` lines, output Latin script.

## Pending

- Grizzly's real-voice check: "test" → Latin "Test"; a Spanish sentence stays Spanish.

## Where we are (handoff 2026-11-03 09:33)

- Done: fallback fix committed `0c8cf89` on main (376 lib tests pass); release v0.9.9 published
  (zip SHA-256 c85083c9…); 0.9.9 installed in /Applications on this Mac (0.9.8 backup in ~/TalknGo-old-backup).
- Done: this Mac needed bun (npm -g) and cmake (brew) to build.
- In progress: macOS permissions reset (tccutil) for com.grizzlytec.talkngo; Grizzly re-adding Accessibility
  and turning on Settings → Advanced → "Limit Auto to My Languages" (downloads the detector).
- Next step: on the other Mac, download and install v0.9.9 from Releases, re-grant permissions, turn on
  "Limit Auto to My Languages", dictate "test" and check the log for `Language detection:` lines.
- Open questions: voice guide sync config (`voice_sync.json`) exists only on the other Mac — copy it here?
  Apple Developer signing (USD 99/yr) so updates stop resetting permissions?

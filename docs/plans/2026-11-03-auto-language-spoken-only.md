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
- Build release 0.9.9, publish on GitHub, install on this Mac.

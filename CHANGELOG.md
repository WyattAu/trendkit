# Changelog

All notable changes. Format: informal, newest first — this is a personal
tool, not a library contract. breaking changes called out inline.

## 0.3.0 — 2026-10-18

- **Engine extracted**: model/store/score/clock/source now live in
  [`signalkit-core`](https://crates.io/crates/signalkit-core); trendkit
  is its first consumer.
- **Integrity signals** (`report --integrity`): fork-ratio flags, org
  bursts, robust velocity outliers — via
  [`signalkit-integrity`](https://crates.io/crates/signalkit-integrity).
  First live findings: a 153.9% fork-ratio repo and a 7-repo org burst
  at 1,269 stars/day combined.
- **HuggingFace source**: trending models via the public Hub API
  (`trendingScore` sort); score = downloads, likes in `extra1`; HF tags
  become topics so the personal profile scores model trends too.
- **AI briefing** (`report --ai`): any OpenAI-compatible endpoint
  (default DeepSeek), key via `ai_api_key`/`TRENDKIT_AI_API_KEY`;
  absent key skips gracefully. Briefing leads the Telegram digest.
- Trending source now records forks consistently with the Search source
  (stars-today in `extra2`), making cross-origin fork ratios trustworthy.

## 0.2.0 — 2026-10-18

- **Unified store (schema v2).** One `subjects`/`observations` pair for
  every source; v1 `repos`/`snapshots` databases migrate automatically
  (legacy history dropped — it predated real velocity anyway).
- **HN source** (Algolia, no auth): windowed trending stories + front
  page. Score = points.
- **crates.io source**: most-downloaded listing, `Δdownloads/day` as the
  trend signal; identifying User-Agent per crates.io policy, 1.2s pacing.
- **Trending-page scrape**: `github.com/trending` has no API, so the
  daily page is parsed as HTML (scraper/html5ever). Observations merge
  into the same `owner/repo` subject space as Search-API rows; `extra1`
  carries "stars today". Article count on the anonymous page varies
  (13–25); the parser takes what is structurally there.
- **Telegram digest**: `report --telegram` via the bot API; token from
  `TRENDKIT_TELEGRAM_TOKEN` or the fleet token file; 4096-char
  truncation on line boundaries. Fleet units in `deploy/`.
- `collect --source all|github|hn|cratesio|trending`.

## 0.1.0 — 2026-10-17

- GitHub Search source (repos created in window, stars-sorted), SQLite
  snapshot store, velocity scoring, personal relevance profile from the
  user's own repositories.
- Dogfood baseline: fetch-kit (dual client policies), retry-backoff
  (search 403/429 schedule), config-kit (layered settings + redacted
  tokens), clock-kit (injected time, MockClock tests).

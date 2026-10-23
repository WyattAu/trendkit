# Changelog

All notable changes. Format: informal, newest first — this is a personal
tool, not a library contract. breaking changes called out inline.

## 0.3.5 / core 0.3.1 — 2026-10-20

- **HF metadata tags filtered**: `region:`/`license:`/`arxiv:`/`doi:` tags
  no longer pollute topic aggregation (they were the top "topics" — 82
  repos of `region:us`).
- **Trust score recalibrated** (signalkit-integrity 0.2.3): the velocity
  outlier penalty is capped at 10 points — a genuinely viral repo
  (z=58) scored trust 0 under the old linear penalty. Popularity is not
  dishonesty.

## 0.3.8 — 2026-10-21

- `report --out <path>`: write the rendered report (incl. integrity,
  topics, persistent, AI sections) to a file — the weekly public page.
- (0.3.5-0.3.7 changelog entries live in the git tags.)

## 0.3.11 — 2026-10-22

- `report --shapes N`: per-subject shape classification
  (accelerating/steady/fading) from per-day velocity pairs
  (signalkit-integrity 0.2.7 `classify_shape`, signalkit-core 0.1.2
  `daily_scores`).
- COOLING label: shrinking-but-positive daily deltas read as cooling,
  not fading.
- `signalkit-integrity 0.2.6`: org bursts catch two-repo
  high-velocity orgs; `name_clones` for cross-org clone farms.

## 0.3.5 / 0.3.6 / 0.3.7 — 2026-10-19/20

- 0.3.5: HF metadata-tag filter, trust recalibration (outlier cap),
  core 0.3.1.
- 0.3.6: same-name clone-farm detection; two-repo high-velocity burst
  rule (signalkit-integrity 0.2.5/0.2.6).
- 0.3.7: `report --persistent N` persistence section (signalkit-core
  0.1.1).

## 0.3.4 — 2026-10-19

- **Trending topics** (`report --topics N`): stars-delta aggregated per
  topic across the window; first run put `mcp` on top (8 repos, +666).
- **Integrity sections** (`report --integrity`): fork-ratio flags now
  carry the composite **trust score and band**; new "trust suspects"
  section (composite < 80, worst first). Band cap: an extreme fork
  verdict holds the band at Watch — no single signal may be ignored.
- **Kind filter** (`report --kinds gh,hn,cr,hf`): filters against the
  full pool, not the truncated top-N.
- **Fix**: descriptions with embedded newlines split markdown table
  rows; sanitized (found by the fleet dress rehearsal).

## 0.3.3 — 2026-10-19

- `report --integrity` (fork flags, org bursts, velocity outliers) and
  the HuggingFace + AI-briefing features' first release.

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

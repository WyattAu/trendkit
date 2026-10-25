# Changelog

All notable changes. Format: informal, newest first — this is a personal
tool, not a library contract. breaking changes called out inline.

## 0.6.1 — 2026-10-25

- Fix: a leftover `edgar` entry in the `--source all` list survived
  the 0.6.0 removal; `collect --source all` would have exited unknown
  source at the next scheduled run. Caught by the post-removal sweep,
  verified by a full live five-source pass.

## 0.6.0 — 2026-10-25

- **EDGAR source removed; business scraping moved back to bizkit**
  where it belongs as a separate scraper with its own store, queries,
  and cadence. Layer correction: trendkit tracks dev trends (github,
  hn, crates.io, trending, huggingface); bizkit tracks SEC filings.
  trendkit-core drops the bizkit-core dependency and the edgar
  module; signalkit-core 0.3.0 drops SourceKind::Edgar. The edgar
  subjects, observations, and forecasts are purged from the store.
- bizkit gains the daily watch command and its own fleet timer (see
  the bizkit repo and chezmoi fleet config).

## 0.5.2 — 2026-10-25

- EDGAR watchlist gains `acquisition of` (bizkit's highest-yield query,
  100 hits in the Oct 24 baseline crawl) as a third watch query, so the
  daily series stays comparable with the pre-trendkit baseline. First
  three-query pass: 300 filings, 159 filers. The window-count score is
  idempotent under same-day re-runs (daily score = max of passes).

## 0.5.1 — 2026-10-25

- **Trust index page**: `report --trust-page <path>` renders a
  standalone public page from the snapshot store — flagged subjects
  worst-first (97 above the Elevated line today, top of the list at a
  226 percent fork ratio), the `.github` name-cluster farm with its
  day-by-day size, and the full method. First release:
  `reports/TRUST_INDEX.md`.
- signalkit-core 0.2.3 `fork_cluster_days`: the cluster curve counts
  each member's last known state per day, not same-day observations —
  sources see farm members sporadically, and same-day counting made
  the farm look like it shrank whenever coverage dipped.

## 0.5.0 — 2026-10-25

- **The forecast loop is closed.** Every projection is now persisted
  (signalkit-core 0.2.1 `forecasts`, unique per target day and
  subject) and scored the next morning against the observed delta:
  MAE and band-hit rate, overall and per kind, rendered as a
  verification block in the digest. Forecasts are falsifiable
  evidence now, not vaporized Telegram lines. First batch: 753
  forecasts persisted for 2026-10-26.
- **EDGAR is the sixth source** (signalkit-core 0.2.0
  `SourceKind::Edgar`, trendkit-core `EdgarSource` on bizkit-core's
  full-text-search client). Each pass runs two curated M&A watch
  queries (merger agreement, tender offer) over the rolling window
  and records per-filer window counts; the day-over-day delta is
  filings velocity. Idempotent by construction: the count is
  window-computed, never accumulated. First pass: 200 filings, 88
  filers. Spread multiplier starts at 2.0 pending held-out receipts.
- Weekly seasonality observed, not yet modeled: Sunday crate
  downloads dip enough that the walk estimator honestly projects the
  dip (syn +6.2M on Sunday vs +14.5M midweek). Monday runs will
  overpredict if the pattern holds; the verification block will
  measure it instead of guessing.
- Fleet unit gains `--forecast 6` (chezmoi side); digests now carry
  projections and their verification.

## 0.4.1 — 2026-10-25

- **Digest attempts are now recorded** (signalkit-core 0.1.5
  `digest_runs`): the report step is fail-soft in the fleet unit
  (ExecStartPost with `-`), so a broken digest never shows in the unit
  status, and journald kept no entries to diagnose it. Every attempt
  now writes (ts, ok, detail) to `digest_runs` — success as ok=1,
  failure with the error chain truncated to 300 chars. Failures still
  exit 1 for systemd, but the cause becomes queryable the same hour
  sys-check runs.
- sys-check (fleet side, separate repo) alerts when the newest digest
  attempt is missing, older than 48h, or failed with its detail.
- Day 8 context: today's 08:13 digest failed root-side with no
  surviving logs; collection succeeded. With this instrumentation the
  next failure carries its own evidence.

## 0.4.0 — 2026-10-24

- **Forecast estimator upgraded, bands calibrated** (signalkit-core
  0.1.4 `project_next_walk`): leave-one-day-out backtests over ~950
  live subjects showed the recency-weighted mean is the worst predictor
  for every kind — it lags trending cumulative counters. Per-kind
  estimator now: crate/github/hn project as a random walk (tomorrow =
  today's delta), hf keeps the mean (backtest: mean wins on MAE 36.9k
  vs 43.7k and coverage 48 vs 29 percent there — HF dynamics are
  noisy-multiplicative, not trending). MAE improvements: crate 928k to
  251k, github 55 to 34, hn 16.5 to 0.5.
- **Per-kind spread multipliers** from the same LOO calibration, fit
  on five days and scored on the held-out day: crate 1.1 (held-out
  coverage 5 percent at the old mean fit, 51 percent at the walk fit),
  github 1.9 (74 percent), hn 1.0 floor (77 percent), hf 1.5 (48
  percent). Bands never claim false precision: multipliers floored at
  1.0. Provisional by construction — recalibrate weekly as data
  accumulates.
- This closes the calibration receipt opened in 0.3.15: instead of
  widening all bands blindly, kinds whose errors are stationary get
  static multipliers (github, hn) and kinds whose errors are not
  (crate acceleration, hf multiplicative noise) get a drift-tracking
  estimator first, multiplier second.

## 0.3.15 — 2026-10-24

- **Forecast time-order fix**: `forecast_section` sorted each subject's
  daily scores by value before differencing. For monotonic counters
  (stars, downloads) value order coincides with day order and nothing
  changed, but any non-monotonic series (an HN story that dips and
  recovers, an HF model whose downloads fluctuate) had its timeline
  scrambled, projecting from fabricated deltas. Deltas now follow time
  order as returned by `daily_scores`. Regression tests cover the
  non-monotonic case, the monotonic invariance, and the store-to-section
  path. Backtest over 7 days of data: on the 20 HF models where the bug
  bit, mean absolute error drops from 90.8k to 40.9k projected delta.
- Band calibration receipt (report, not code): the ±1 sigma spread band
  under-covers for every kind. A ~68 percent band needs roughly 2-3x the
  current spread (crates 2.9x, github 2.2x, hn 1.4x, hf 3.3x), and HF
  download dynamics are multiplicative (median relative error 236
  percent). A per-kind band or log-scale projection is the honest 0.4.x
  change; not patched hastily on one week of data.
- **0.3.14 yanked**: a concurrent working tree published 0.3.14 twelve
  minutes before this one, carrying 0.3.13's source under a new version
  number and missing this fix. It matched no git tag, so it was yanked
  to keep crates.io and git honest with each other. The version-number
  collisions (0.3.13, 0.3.14) both came from un-synced working trees —
  the fix is process, not code: version bumps happen in one tree only.

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

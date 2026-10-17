# trendkit

Dev-trend crawler in Rust: snapshot GitHub trending signals into SQLite,
score star velocity over time windows, and rank results by *personal*
relevance against your own repository profile.

```
trendkit collect                 # snapshot trending new repos + refresh profile
trendkit report --top 25         # velocity table
trendkit report --personal -md   # markdown, ranked by relevance to your repos
trendkit profile update          # rebuild profile from WyattAu/* repos
trendkit profile show
```

## Design

- **Snapshot-first.** GitHub exposes no star-history API and no official
  trending API; honest velocity requires your own snapshots over time.
  Every `collect` run appends immutable observations; `report` computes
  `Δstars/day` over a window, clamped to one hour minimum.
- **Dogfooded.** Built on the maintainer's own published crates:
  `fetch-kit` (HTTP), `retry-backoff` (search rate-limit schedule),
  `config-kit` (layered settings, redacted tokens), `clock-kit`
  (injectable time for deterministic tests), plus `trendkit-core` as the
  shared seam for future sources (`hn`, `cratesio`).
- **Two clients, two policies.** The GitHub Search API (30 req/min
  authenticated) gets manual pacing plus exponential backoff on 403/429
  up to 60s; the core REST API (5000 req/h) keeps fetch-kit's built-in
  retries.
- **Tokens never print.** `config-kit`'s `Sensitive<String>` renders
  `[REDACTED]`; `.expose()` is the only way in.

## Configuration

Precedence: CLI flags > `TRENDKIT_*` env > `trendkit.toml` > defaults.
Unknown top-level keys in the file are rejected (strict load).

```toml
# trendkit.toml
db_path       = "trendkit.db"
github_owner  = "WyattAu"
github_pages  = 3
window_days   = 14
# github_token = "ghp_..."   # or TRENDKIT_GITHUB_TOKEN / GH_TOKEN env
```

## Status

v0.1 — milestone ①: GitHub source, SQLite snapshot store, velocity
scoring, personal profile. HN/crates.io sources, trending-page HTML
scrape (via `crawlkit`), and Telegram digests are planned milestones.

Velocity is only meaningful after ≥ 2 collection runs; until then the
report shows zero velocity by design rather than fabricating spikes from
a single observation.

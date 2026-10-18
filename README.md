# trendkit

[![CI](https://github.com/WyattAu/trendkit/actions/workflows/ci.yml/badge.svg)](https://github.com/WyattAu/trendkit/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.95+-orange.svg)](https://www.rust-lang.org/)

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

v0.2 — GitHub Search + HN + crates.io sources over one unified
subject/observation store (schema v2, migrated from v1), trending-page
HTML scrape, velocity scoring, personal relevance, Telegram digest
(`report --telegram`, token via `/etc/alertmanager/telegram-token` per
fleet convention). Scheduled collection: `deploy/trendkit-collect.*`
systemd units.

The `crawlkit-engine` library integration is deliberately deferred: its
default feature set pulls wasmtime/sqlx/redis for a single HTML GET.
Revisit when crawlkit ships a lean fetch facade.

Velocity is only meaningful after ≥ 2 collection runs; until then the
report shows zero velocity by design rather than fabricating spikes from
a single observation.

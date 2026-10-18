# trendkit-core

Core types, SQLite snapshot store, source traits, and velocity scoring
for [trendkit](https://github.com/WyattAu/trendkit).

- **Subjects/observations**: one append-only snapshot model shared by all
  sources (GitHub repos, HN stories, crates.io crates).
- **Velocity**: `Δscore/day` over a window, clamped to one hour minimum —
  single-snapshot subjects report zero rather than a fabricated spike.
- **Deterministic time**: all timestamps flow through an injected
  `clock_kit::Clock`, so collectors are testable with `MockClock`.
- **Dogfooded**: fetch-kit transport, retry-backoff schedules,
  clock-kit time.

See the [workspace README](https://github.com/WyattAu/trendkit) for the
CLI and full design notes.

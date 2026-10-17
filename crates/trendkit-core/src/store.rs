//! SQLite snapshot store.
//!
//! Schema is migrated lazily on [`Store::open`] / [`Store::open_in_memory`]
//! and is append-only: `repos` holds identity, `snapshots` holds immutable
//! observations keyed by `(repo_id, ts)`, and `collect_runs` records each
//! collection pass for later honesty checks ("is this DB real history or
//! one backfill?").

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::{Result, TrendError};
use crate::model::{RepoListed, RepoRecord, RepoSnapshot, VelocityRow};
use crate::score;

/// Topics are joined with this separator inside a single TEXT column.
pub const TOPIC_SEP: char = '\u{0}';

/// A handle to the trendkit SQLite database.
///
/// The connection sits behind a mutex so a [`Store`] is `Send + Sync`:
/// sources run sequentially today, but milestone ② runs them concurrently
/// against one store. Individual statements are short; no lock is held
/// across `.await` points.
#[derive(Debug)]
pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    /// Open (and migrate) the database at `path`.
    ///
    /// # Errors
    /// Fails when the file cannot be opened or the schema cannot migrate.
    pub fn open(path: &Path) -> Result<Store> {
        let conn = Connection::open(path).map_err(TrendError::from)?;
        let store = Store {
            conn: Mutex::new(conn),
        };
        store.configure_and_migrate()?;
        Ok(store)
    }

    /// Open an in-memory database (tests and `--dry-run` style reporting).
    ///
    /// # Errors
    /// Fails when the in-memory database cannot be created or migrated.
    pub fn open_in_memory() -> Result<Store> {
        let conn = Connection::open_in_memory().map_err(TrendError::from)?;
        let store = Store {
            conn: Mutex::new(conn),
        };
        store.configure_and_migrate()?;
        Ok(store)
    }

    /// Lock the connection; poisoning maps to a typed error.
    fn lock(&self) -> Result<MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|_| TrendError::Other("store mutex poisoned".to_owned()))
    }

    fn configure_and_migrate(&self) -> Result<()> {
        let conn = self.lock()?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(TrendError::from)?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(TrendError::from)?;
        conn.execute_batch(
            "
                CREATE TABLE IF NOT EXISTS repos (
                    repo_id     INTEGER PRIMARY KEY,
                    full_name   TEXT NOT NULL UNIQUE,
                    owner       TEXT NOT NULL,
                    name        TEXT NOT NULL,
                    description TEXT,
                    language    TEXT,
                    html_url    TEXT,
                    created_at  TEXT,
                    topics      TEXT NOT NULL DEFAULT ''
                );
                CREATE TABLE IF NOT EXISTS snapshots (
                    repo_id     INTEGER NOT NULL REFERENCES repos(repo_id),
                    ts          INTEGER NOT NULL,
                    stars       INTEGER NOT NULL,
                    forks       INTEGER NOT NULL,
                    watchers    INTEGER NOT NULL,
                    open_issues INTEGER NOT NULL,
                    PRIMARY KEY (repo_id, ts)
                );
                CREATE TABLE IF NOT EXISTS profile_repos (
                    full_name   TEXT PRIMARY KEY,
                    language    TEXT,
                    topics      TEXT NOT NULL DEFAULT '',
                    description TEXT,
                    is_fork     INTEGER NOT NULL DEFAULT 0,
                    pushed_at   TEXT,
                    stars       INTEGER NOT NULL DEFAULT 0
                );
                CREATE TABLE IF NOT EXISTS collect_runs (
                    run_id  INTEGER PRIMARY KEY,
                    ts      INTEGER NOT NULL,
                    source  TEXT NOT NULL,
                    pages   INTEGER NOT NULL,
                    items   INTEGER NOT NULL
                );
                ",
        )
        .map_err(TrendError::from)?;
        Ok(())
    }

    /// Insert or update a repository identity row; returns its `repo_id`.
    ///
    /// # Errors
    /// Propagates SQLite failures.
    pub fn upsert_repo(&self, repo: &RepoRecord) -> Result<i64> {
        let sep = TOPIC_SEP.to_string();
        let topics = repo.topics.join(&sep);
        let conn = self.lock()?;
        conn.query_row(
                "
                INSERT INTO repos (full_name, owner, name, description, language, html_url, created_at, topics)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                ON CONFLICT(full_name) DO UPDATE SET
                    description = excluded.description,
                    language    = excluded.language,
                    html_url    = excluded.html_url,
                    created_at  = COALESCE(excluded.created_at, repos.created_at),
                    topics      = excluded.topics
                RETURNING repo_id
                ",
                params![
                    repo.full_name,
                    repo.owner,
                    repo.name,
                    repo.description,
                    repo.language,
                    repo.html_url,
                    repo.created_at,
                    topics,
                ],
                |row| row.get(0),
            )
            .map_err(TrendError::from)
    }

    /// Append one snapshot observation for `repo_id`.
    ///
    /// # Errors
    /// Propagates SQLite failures (including missing foreign key).
    pub fn insert_snapshot(&self, repo_id: i64, snap: &RepoSnapshot) -> Result<()> {
        let conn = self.lock()?;
        conn.execute(
            "
                INSERT OR REPLACE INTO snapshots (repo_id, ts, stars, forks, watchers, open_issues)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                ",
            params![
                repo_id,
                snap.ts,
                snap.stars,
                snap.forks,
                snap.watchers,
                snap.open_issues
            ],
        )
        .map_err(TrendError::from)?;
        Ok(())
    }

    /// Record a completed collection pass.
    ///
    /// # Errors
    /// Propagates SQLite failures.
    pub fn record_run(&self, ts: i64, source: &str, pages: i64, items: i64) -> Result<()> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO collect_runs (ts, source, pages, items) VALUES (?1, ?2, ?3, ?4)",
            params![ts, source, pages, items],
        )
        .map_err(TrendError::from)?;
        Ok(())
    }

    /// The most recent collection pass, if any.
    ///
    /// # Errors
    /// Propagates SQLite failures.
    pub fn last_run(&self) -> Result<Option<(i64, String, i64)>> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT ts, source, items FROM collect_runs ORDER BY run_id DESC LIMIT 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(TrendError::from)
    }

    /// `(repo count, snapshot count)` for report headers.
    ///
    /// # Errors
    /// Propagates SQLite failures.
    pub fn counts(&self) -> Result<(i64, i64)> {
        let conn = self.lock()?;
        let repos: i64 = conn
            .query_row("SELECT COUNT(*) FROM repos", [], |row| row.get(0))
            .map_err(TrendError::from)?;
        let snaps: i64 = conn
            .query_row("SELECT COUNT(*) FROM snapshots", [], |row| row.get(0))
            .map_err(TrendError::from)?;
        Ok((repos, snaps))
    }

    /// Highest-velocity repositories over the last `window_days` days.
    ///
    /// Repositories with a single snapshot inside the window are included
    /// with zero velocity rather than skipped, so early-stage databases
    /// still show what is being tracked.
    ///
    /// # Errors
    /// Propagates SQLite failures.
    pub fn velocity_rows(&self, window_days: u32, limit: u32) -> Result<Vec<VelocityRow>> {
        let window_secs = i64::from(window_days) * 86_400;
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "
                WITH latest AS (
                    SELECT repo_id, MAX(ts) AS ts FROM snapshots GROUP BY repo_id
                ),
                earliest AS (
                    SELECT s.repo_id, MIN(s.ts) AS ts
                    FROM snapshots s
                    JOIN latest l ON l.repo_id = s.repo_id
                    WHERE s.ts >= l.ts - ?1
                    GROUP BY s.repo_id
                )
                SELECT r.full_name, r.language, r.description, r.topics,
                       ls.stars, es.stars, (ls.ts - es.ts)
                FROM latest l
                JOIN earliest e ON e.repo_id = l.repo_id
                JOIN snapshots ls ON ls.repo_id = l.repo_id AND ls.ts = l.ts
                JOIN snapshots es ON es.repo_id = e.repo_id AND es.ts = e.ts
                JOIN repos r ON r.repo_id = l.repo_id
                ORDER BY (ls.stars - es.stars) * 1.0 / MAX((ls.ts - es.ts), 3600) DESC
                LIMIT ?2
                ",
            )
            .map_err(TrendError::from)?;

        let rows = stmt
            .query_map(params![window_secs, i64::from(limit)], |row| {
                let full_name: String = row.get(0)?;
                let language: Option<String> = row.get(1)?;
                let description: Option<String> = row.get(2)?;
                let topics_raw: String = row.get(3)?;
                let stars_now: i64 = row.get(4)?;
                let stars_prev: i64 = row.get(5)?;
                let window: i64 = row.get(6)?;
                Ok(VelocityRow {
                    full_name,
                    language,
                    description,
                    topics: split_topics(&topics_raw),
                    stars_now,
                    stars_prev,
                    window_secs: window,
                    velocity_per_day: score::velocity_per_day(stars_prev, stars_now, window),
                })
            })
            .map_err(TrendError::from)?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(TrendError::from)?);
        }
        Ok(out)
    }

    /// Replace the stored personal profile with `rows`.
    ///
    /// # Errors
    /// Propagates SQLite failures.
    pub fn save_profile_rows(&self, rows: &[RepoListed]) -> Result<()> {
        let conn = self.lock()?;
        conn.execute("DELETE FROM profile_repos", [])
            .map_err(TrendError::from)?;
        for r in rows {
            let topics = r.topics.join(&TOPIC_SEP.to_string());
            conn.execute(
                    "
                    INSERT INTO profile_repos (full_name, language, topics, description, is_fork, pushed_at, stars)
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                    ",
                    params![
                        r.full_name,
                        r.language,
                        topics,
                        r.description,
                        i64::from(r.is_fork),
                        r.pushed_at,
                        r.stars,
                    ],
                )
                .map_err(TrendError::from)?;
        }
        Ok(())
    }

    /// The stored personal profile rows, if any.
    ///
    /// # Errors
    /// Propagates SQLite failures.
    pub fn load_profile_rows(&self) -> Result<Vec<RepoListed>> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT full_name, language, topics, description, is_fork, pushed_at, stars
                 FROM profile_repos ORDER BY full_name",
            )
            .map_err(TrendError::from)?;
        let rows = stmt
            .query_map([], |row| {
                let full_name: String = row.get(0)?;
                let language: Option<String> = row.get(1)?;
                let topics_raw: String = row.get(2)?;
                let description: Option<String> = row.get(3)?;
                let is_fork: i64 = row.get(4)?;
                let pushed_at: Option<String> = row.get(5)?;
                let stars: i64 = row.get(6)?;
                Ok(RepoListed {
                    full_name,
                    language,
                    topics: split_topics(&topics_raw),
                    description,
                    is_fork: is_fork != 0,
                    pushed_at,
                    stars,
                })
            })
            .map_err(TrendError::from)?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(TrendError::from)?);
        }
        Ok(out)
    }
}

/// Split the internal topic-joined column back into topics.
fn split_topics(raw: &str) -> Vec<String> {
    if raw.is_empty() {
        Vec::new()
    } else {
        raw.split(TOPIC_SEP).map(str::to_owned).collect()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn record(name: &str, lang: Option<&str>, topics: &[&str]) -> RepoRecord {
        RepoRecord {
            full_name: name.to_owned(),
            owner: name.split('/').next().unwrap_or(name).to_owned(),
            name: name.to_owned(),
            description: Some("desc".to_owned()),
            language: lang.map(str::to_owned),
            html_url: None,
            created_at: None,
            topics: topics.iter().map(|t| (*t).to_owned()).collect(),
        }
    }

    fn snap(ts: i64, stars: i64) -> RepoSnapshot {
        RepoSnapshot {
            ts,
            stars,
            forks: 1,
            watchers: 1,
            open_issues: 0,
        }
    }

    #[test]
    fn upsert_is_idempotent_on_full_name() {
        let store = Store::open_in_memory().unwrap();
        let id1 = store
            .upsert_repo(&record("a/b", Some("Rust"), &["cli"]))
            .unwrap();
        let id2 = store
            .upsert_repo(&record("a/b", Some("Rust"), &["cli"]))
            .unwrap();
        assert_eq!(id1, id2);
        let (repos, snaps) = store.counts().unwrap();
        assert_eq!(repos, 1);
        assert_eq!(snaps, 0);
    }

    #[test]
    fn velocity_ranks_fastest_first() {
        let store = Store::open_in_memory().unwrap();
        let slow = store
            .upsert_repo(&record("s/slow", Some("Go"), &[]))
            .unwrap();
        let fast = store
            .upsert_repo(&record("f/fast", Some("Rust"), &["wasm"]))
            .unwrap();
        store.insert_snapshot(slow, &snap(0, 100)).unwrap();
        store.insert_snapshot(slow, &snap(86_400, 110)).unwrap(); // +10/day
        store.insert_snapshot(fast, &snap(0, 100)).unwrap();
        store.insert_snapshot(fast, &snap(86_400, 170)).unwrap(); // +70/day

        let rows = store.velocity_rows(14, 10).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].full_name, "f/fast");
        assert!((rows[0].velocity_per_day - 70.0).abs() < 1e-9);
        assert!((rows[1].velocity_per_day - 10.0).abs() < 1e-9);
        assert_eq!(rows[0].topics, vec!["wasm".to_owned()]);
    }

    #[test]
    fn single_snapshot_reports_zero_velocity() {
        let store = Store::open_in_memory().unwrap();
        let id = store.upsert_repo(&record("o/only", None, &[])).unwrap();
        store.insert_snapshot(id, &snap(86_400, 5_000)).unwrap();
        let rows = store.velocity_rows(14, 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert!((rows[0].velocity_per_day - 0.0).abs() < 1e-9);
        assert_eq!(rows[0].stars_now, 5_000);
        assert_eq!(rows[0].stars_prev, 5_000);
    }

    #[test]
    fn profile_roundtrip_preserves_topics() {
        let store = Store::open_in_memory().unwrap();
        let rows = vec![RepoListed {
            full_name: "WyattAu/crawlkit".to_owned(),
            language: Some("Rust".to_owned()),
            topics: vec!["crawler".to_owned(), "seo".to_owned()],
            description: None,
            is_fork: false,
            pushed_at: None,
            stars: 42,
        }];
        store.save_profile_rows(&rows).unwrap();
        let loaded = store.load_profile_rows().unwrap();
        assert_eq!(loaded, rows);
    }

    #[test]
    fn runs_are_recorded_and_latest_read_back() {
        let store = Store::open_in_memory().unwrap();
        assert!(store.last_run().unwrap().is_none());
        store.record_run(1, "github", 3, 300).unwrap();
        store.record_run(2, "github", 2, 200).unwrap();
        let Some((ts, source, items)) = store.last_run().unwrap() else {
            panic!("expected a recorded run");
        };
        assert_eq!((ts, source.as_str(), items), (2, "github", 200));
    }
}

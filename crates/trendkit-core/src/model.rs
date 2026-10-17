//! Shared data model for snapshot collection and reporting.

/// A repository's identity, stored once per repo and updated on later scans.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoRecord {
    /// GitHub full name (`owner/repo`).
    pub full_name: String,
    /// Owner login, derived from `full_name` when the payload omits it.
    pub owner: String,
    /// Repository name.
    pub name: String,
    /// Short description, if any.
    pub description: Option<String>,
    /// Primary language, if any.
    pub language: Option<String>,
    /// Canonical URL.
    pub html_url: Option<String>,
    /// Creation timestamp as returned by GitHub (RFC 3339), if present.
    pub created_at: Option<String>,
    /// Repository topics, lowercased by GitHub.
    pub topics: Vec<String>,
}

/// A point-in-time observation of one repository's counters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoSnapshot {
    /// Unix seconds at observation time.
    pub ts: i64,
    /// Star count.
    pub stars: i64,
    /// Fork count.
    pub forks: i64,
    /// Watcher count.
    pub watchers: i64,
    /// Open issue (and PR) count.
    pub open_issues: i64,
}

/// One row from a user's repository listing, used for the personal profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoListed {
    /// GitHub full name (`owner/repo`).
    pub full_name: String,
    /// Primary language, if any.
    pub language: Option<String>,
    /// Repository topics, lowercased by GitHub.
    pub topics: Vec<String>,
    /// Short description, if any.
    pub description: Option<String>,
    /// True for forks (excluded from profile weighting).
    pub is_fork: bool,
    /// Last push timestamp as returned by GitHub, if present.
    pub pushed_at: Option<String>,
    /// Star count at listing time.
    pub stars: i64,
}

/// A velocity computation for one repository over a snapshot window.
#[derive(Debug, Clone, PartialEq)]
pub struct VelocityRow {
    /// GitHub full name (`owner/repo`).
    pub full_name: String,
    /// Primary language, if any.
    pub language: Option<String>,
    /// Short description, if any.
    pub description: Option<String>,
    /// Topics attached to the repository.
    pub topics: Vec<String>,
    /// Most recent observed star count.
    pub stars_now: i64,
    /// Earliest observed star count inside the window.
    pub stars_prev: i64,
    /// Seconds between the two observations.
    pub window_secs: i64,
    /// Stars gained per day over the window (see [`crate::score`]).
    pub velocity_per_day: f64,
}

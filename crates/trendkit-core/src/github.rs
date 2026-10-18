//! GitHub source: Search-API trending collection and user repo listing.
//!
//! Two clients, two policies, on purpose:
//!
//! - `repos` client keeps fetch-kit's built-in retry (3 attempts) — the
//!   core REST API allows 5000 req/h and transient failures are cheap.
//! - `search` client disables built-in retry and drives its own loop:
//!   requests are paced to the authenticated Search budget (30 req/min)
//!   and 403/429 answers back off via `retry-backoff`, whose exponential
//!   schedule reaches 60s — beyond fetch-kit's 30s built-in max.

use std::time::Duration;

use fetch_kit::{Client, FetchError};
use retry_backoff::backoffs::impl_exponential_backoff::Backoff;
use retry_backoff::RetryBackoff;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::clock::{iso_date_from_unix, unix_seconds};
use crate::error::{Result, TrendError};
use crate::model::{Observation, RepoListed, SourceKind, SubjectRecord};
use crate::source::{CollectCtx, CollectReport, TrendSource, SEARCH_PACING};

const API_ROOT: &str = "https://api.github.com";
const SEARCH_PATH: &str = "/search/repositories";
const USER_AGENT: &str = "trendkit/0.2";
const API_VERSION: &str = "2022-11-28";
const PER_PAGE: u32 = 100;
/// Attempts per search page before declaring the endpoint rate-limited.
const RATE_LIMIT_ATTEMPTS: u32 = 4;
/// Cap for profile listings: 5 pages x 100 covers 500 repos.
const USER_REPOS_MAX_PAGES: u32 = 5;

/// GitHub trend source.
#[derive(Debug, Clone)]
pub struct GithubSource {
    search: Client,
    repos: Client,
    token: Option<String>,
    backoff: Backoff,
}

impl GithubSource {
    /// Build a source; `token` is a GitHub PAT (env `GH_TOKEN` in the CLI).
    #[must_use]
    pub fn new(token: Option<String>) -> GithubSource {
        let search = Client::builder()
            .user_agent(USER_AGENT)
            // HeaderName::from_static demands lowercase — uppercase panics.
            .default_header("accept", "application/vnd.github+json")
            .default_header("x-github-api-version", API_VERSION)
            .timeout(Duration::from_secs(30))
            .retries(0)
            .build();
        let repos = Client::builder()
            .user_agent(USER_AGENT)
            .default_header("accept", "application/vnd.github+json")
            .default_header("x-github-api-version", API_VERSION)
            .timeout(Duration::from_secs(30))
            .build();
        let backoff = Backoff::new(Duration::from_secs(2), Some(Duration::from_secs(60)));
        GithubSource {
            search,
            repos,
            token,
            backoff,
        }
    }

    /// Collect trending new repos: created within the window, sorted by stars.
    async fn search_trending(&self, ctx: &CollectCtx) -> Result<usize> {
        let now = unix_seconds(ctx.clock.as_ref());
        let since = iso_date_from_unix(now - i64::from(ctx.window_days) * 86_400);
        let query = format!("created:>{since}");
        let mut total = 0usize;

        for page in 1..=ctx.pages {
            let params = SearchQuery {
                q: &query,
                sort: "stars",
                order: "desc",
                per_page: PER_PAGE,
                page,
            };
            let response: SearchResponse = self.search_get(SEARCH_PATH, &params).await?;
            let count = response.items.len();
            for item in response.items {
                let observation = Observation {
                    ts: now,
                    score: item.stargazers_count,
                    extra1: item.forks_count,
                    extra2: item.watchers_count,
                };
                let subject = SubjectRecord::from(item);
                let subject_id = ctx.store.upsert_subject(&subject)?;
                ctx.store.insert_observation(subject_id, &observation)?;
                total += 1;
            }
            if count < PER_PAGE as usize || page == ctx.pages {
                break;
            }
            tokio::time::sleep(SEARCH_PACING).await;
        }

        ctx.store
            .record_run(now, self.name(), i64::from(ctx.pages), total as i64)?;
        Ok(total)
    }

    /// GET with search-endpoint rate-limit handling (403/429 → backoff).
    async fn search_get<Q, T>(&self, path: &str, params: &Q) -> Result<T>
    where
        Q: Serialize,
        T: DeserializeOwned,
    {
        let url = format!("{API_ROOT}{path}");
        let mut attempt: u32 = 0;
        loop {
            attempt += 1;
            let mut request = self.search.get(&url).query(params);
            if let Some(token) = &self.token {
                request = request.bearer_auth(token);
            }
            match request.json_response::<T>().await {
                Ok(value) => return Ok(value),
                Err(FetchError::StatusCode {
                    status: 403 | 429, ..
                }) => {
                    if attempt >= RATE_LIMIT_ATTEMPTS {
                        return Err(TrendError::RateLimited {
                            endpoint: "search/repositories".to_owned(),
                            attempts: RATE_LIMIT_ATTEMPTS,
                        });
                    }
                    let wait = RetryBackoff::delay(&self.backoff, attempt as usize);
                    tracing::warn!(
                        attempt,
                        wait_secs = wait.as_secs(),
                        "github search rate limited; backing off"
                    );
                    tokio::time::sleep(wait).await;
                }
                Err(err) => return Err(TrendError::Fetch(err)),
            }
        }
    }

    /// List a user's own (non-fork-inclusive) repos for the personal profile.
    ///
    /// Core REST endpoint: fetch-kit's built-in retry applies, no manual
    /// pacing needed at 5000 req/h.
    pub async fn list_user_repos(&self, owner: &str) -> Result<Vec<RepoListed>> {
        let mut out = Vec::new();
        for page in 1..=USER_REPOS_MAX_PAGES {
            let url = format!("{API_ROOT}/users/{owner}/repos");
            let params = ListQuery {
                per_page: PER_PAGE,
                page,
                sort: "pushed",
                list_type: "owner",
            };
            let response: Vec<RepoJson> = self.repos_get(&url, &params).await?;
            let count = response.len();
            out.extend(response.into_iter().map(RepoListed::from));
            if count < PER_PAGE as usize {
                break;
            }
        }
        Ok(out)
    }

    async fn repos_get<Q, T>(&self, url: &str, params: &Q) -> Result<T>
    where
        Q: Serialize,
        T: DeserializeOwned,
    {
        let mut request = self.repos.get(url).query(params);
        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }
        Ok(request.json_response::<T>().await?)
    }
}

impl TrendSource for GithubSource {
    fn name(&self) -> &'static str {
        "github"
    }

    // The future is deliberately not `Send`: the SQLite connection is
    // confined to the collecting task, and API sources are sequential by
    // design (pacing). Multi-source concurrency happens above one store.
    fn collect(
        &self,
        ctx: &CollectCtx,
    ) -> impl std::future::Future<Output = Result<CollectReport>> {
        let ctx = ctx.clone();
        async move {
            let items = self.search_trending(&ctx).await?;
            Ok(CollectReport {
                source: self.name(),
                items,
                pages: ctx.pages as usize,
            })
        }
    }
}

#[derive(Debug, Serialize)]
struct SearchQuery<'a> {
    q: &'a str,
    sort: &'a str,
    order: &'a str,
    per_page: u32,
    page: u32,
}

#[derive(Debug, Serialize)]
struct ListQuery<'a> {
    per_page: u32,
    page: u32,
    sort: &'a str,
    #[serde(rename = "type")]
    list_type: &'a str,
}

/// Deserialize GitHub search results.
#[derive(Debug, Deserialize)]
struct SearchResponse {
    #[serde(default)]
    items: Vec<RepoJson>,
}

/// The subset of the GitHub repository payload trendkit consumes.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)] // counter fields are payload-derived and kept for fidelity
pub(crate) struct RepoJson {
    #[serde(default)]
    pub(crate) full_name: String,
    #[serde(default)]
    pub(crate) owner: Option<Owner>,
    #[serde(default)]
    pub(crate) name: Option<String>,
    #[serde(default)]
    pub(crate) description: Option<String>,
    #[serde(default)]
    pub(crate) language: Option<String>,
    #[serde(default)]
    pub(crate) html_url: Option<String>,
    #[serde(default)]
    pub(crate) created_at: Option<String>,
    #[serde(default)]
    pub(crate) pushed_at: Option<String>,
    #[serde(default)]
    pub(crate) stargazers_count: i64,
    #[serde(default)]
    pub(crate) forks_count: i64,
    #[serde(default)]
    pub(crate) watchers_count: i64,
    #[serde(default)]
    pub(crate) open_issues_count: i64,
    #[serde(default)]
    pub(crate) fork: bool,
    #[serde(default)]
    pub(crate) topics: Vec<String>,
}

/// `owner` object; only the login is used.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)] // payload-derived; kept for payload fidelity and debugging
pub(crate) struct Owner {
    pub(crate) login: String,
}

impl From<RepoJson> for SubjectRecord {
    fn from(json: RepoJson) -> Self {
        let full_name = json.full_name;
        let url = json.html_url.clone().unwrap_or_else(|| {
            if full_name.is_empty() {
                String::new()
            } else if full_name.contains('/') {
                format!("{GITHUB_WEB_ROOT}/{full_name}")
            } else {
                // Degenerate payload without an owner object: synthesize
                // a same-name owner path so the subject still gets a URL.
                format!("{GITHUB_WEB_ROOT}/{full_name}/{full_name}")
            }
        });
        SubjectRecord {
            kind: SourceKind::GithubRepo,
            key: full_name,
            title: json.description,
            lang: json.language,
            url: Some(url),
            topics: json.topics,
            created_at: json.created_at,
        }
    }
}

/// Web root used to synthesize repo URLs when the payload omits them.
pub const GITHUB_WEB_ROOT: &str = "https://github.com";

impl From<RepoJson> for RepoListed {
    fn from(json: RepoJson) -> Self {
        RepoListed {
            full_name: json.full_name,
            language: json.language,
            topics: json.topics,
            description: json.description,
            is_fork: json.fork,
            pushed_at: json.pushed_at,
            stars: json.stargazers_count,
        }
    }
}

/// Parse a GitHub search response body into subjects (used by tests and
/// available to callers who capture raw payloads).
///
/// # Errors
/// Fails when the payload is not a GitHub search response.
pub fn parse_search_json(payload: &str) -> Result<Vec<SubjectRecord>> {
    let response: SearchResponse = serde_json::from_str(payload)
        .map_err(|e| TrendError::Config(format!("invalid search payload: {e}")))?;
    Ok(response
        .items
        .into_iter()
        .map(SubjectRecord::from)
        .collect())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    const SAMPLE: &str = r#"{
        "total_count": 2,
        "incomplete_results": false,
        "items": [
            {
                "id": 1,
                "full_name": "tokio-rs/tokio",
                "name": "tokio",
                "owner": { "login": "tokio-rs" },
                "description": "A runtime for Rust",
                "language": "Rust",
                "html_url": "https://github.com/tokio-rs/tokio",
                "created_at": "2026-10-10T00:00:00Z",
                "pushed_at": "2026-10-16T00:00:00Z",
                "stargazers_count": 42,
                "forks_count": 3,
                "watchers_count": 42,
                "open_issues_count": 5,
                "fork": false,
                "topics": ["async", "runtime"]
            },
            {
                "id": 2,
                "full_name": "sparse-repo",
                "stargazers_count": 7
            }
        ]
    }"#;

    #[test]
    fn parse_extracts_subjects_with_fallbacks() {
        let subjects = parse_search_json(SAMPLE).unwrap();
        assert_eq!(subjects.len(), 2);

        assert_eq!(subjects[0].kind, SourceKind::GithubRepo);
        assert_eq!(subjects[0].key, "tokio-rs/tokio");
        assert_eq!(subjects[0].title.as_deref(), Some("A runtime for Rust"));
        assert_eq!(subjects[0].lang.as_deref(), Some("Rust"));
        assert_eq!(
            subjects[0].url.as_deref(),
            Some("https://github.com/tokio-rs/tokio")
        );
        assert_eq!(
            subjects[0].topics,
            vec!["async".to_owned(), "runtime".to_owned()]
        );

        // Degenerate second item: url synthesized from full_name.
        assert_eq!(subjects[1].key, "sparse-repo");
        assert_eq!(
            subjects[1].url.as_deref(),
            Some("https://github.com/sparse-repo/sparse-repo")
        );
    }

    #[test]
    fn listed_rows_carry_counters() {
        let response: SearchResponse = serde_json::from_str(SAMPLE).unwrap();
        let listed: Vec<RepoListed> = response.items.into_iter().map(RepoListed::from).collect();
        assert_eq!(listed[0].stars, 42);
        assert!(!listed[0].is_fork);
        assert_eq!(listed[0].topics.len(), 2);
    }

    #[test]
    fn parse_rejects_garbage() {
        assert!(parse_search_json("not json").is_err());
    }
}

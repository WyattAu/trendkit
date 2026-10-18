//! crates.io source: most-downloaded crates, snapshotted over time.
//!
//! Download totals are cumulative, so `Δdownloads/day` is the trend
//! signal. The list endpoint is used (no auth); crates.io policy
//! requires an identifying User-Agent, which fetch-kit sets.

use std::time::Duration;

use fetch_kit::Client;
use serde::{Deserialize, Serialize};

use crate::clock::unix_seconds;
use crate::error::Result;
use crate::model::{Observation, SourceKind, SubjectRecord};
use crate::source::{CollectCtx, CollectReport, TrendSource};

const CRATES_API: &str = "https://crates.io/api/v1";
const USER_AGENT: &str = "trendkit/0.2 (https://github.com/WyattAu/trendkit)";
const PER_PAGE: u32 = 100;
/// crates.io asks for <= 1 req/s from identifiable clients.
pub const CRATES_PACING: Duration = Duration::from_millis(1_200);

/// crates.io trend source.
#[derive(Debug, Clone)]
pub struct CratesSource {
    client: Client,
}

impl CratesSource {
    /// Build a source.
    #[must_use]
    pub fn new() -> CratesSource {
        CratesSource {
            client: Client::builder().user_agent(USER_AGENT).build(),
        }
    }
}

impl Default for CratesSource {
    fn default() -> Self {
        CratesSource::new()
    }
}

impl TrendSource for CratesSource {
    fn name(&self) -> &'static str {
        "cratesio"
    }

    fn collect(
        &self,
        ctx: &CollectCtx,
    ) -> impl std::future::Future<Output = Result<CollectReport>> {
        let ctx = ctx.clone();
        async move {
            let now = unix_seconds(ctx.clock.as_ref());
            let mut total = 0usize;
            for page in 1..=ctx.pages {
                let params = CratesQuery {
                    sort: "downloads",
                    per_page: PER_PAGE,
                    page,
                };
                let url = format!("{CRATES_API}/crates");
                let response: CratesResponse =
                    self.client.get(url).query(&params).json_response().await?;
                let count = response.crates.len();
                for krate in response.crates {
                    let observation = Observation {
                        ts: now,
                        score: krate.downloads,
                        extra1: krate.recent_downloads.map(|f| f as i64).unwrap_or(0),
                        extra2: 0,
                    };
                    let subject = SubjectRecord::from(krate);
                    let subject_id = ctx.store.upsert_subject(&subject)?;
                    ctx.store.insert_observation(subject_id, &observation)?;
                    total += 1;
                }
                if count < PER_PAGE as usize || page == ctx.pages {
                    break;
                }
                tokio::time::sleep(CRATES_PACING).await;
            }
            ctx.store
                .record_run(now, self.name(), i64::from(ctx.pages), total as i64)?;
            Ok(CollectReport {
                source: self.name(),
                items: total,
                pages: ctx.pages as usize,
            })
        }
    }
}

#[derive(Debug, Serialize)]
struct CratesQuery<'a> {
    sort: &'a str,
    per_page: u32,
    page: u32,
}

#[derive(Debug, Deserialize)]
struct CratesResponse {
    #[serde(default)]
    crates: Vec<CrateJson>,
}

/// The subset of a crates.io crate summary trendkit consumes.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)] // payload-derived; kept for payload fidelity and debugging
pub(crate) struct CrateJson {
    #[serde(default)]
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) description: Option<String>,
    #[serde(default)]
    pub(crate) downloads: i64,
    #[serde(default)]
    pub(crate) recent_downloads: Option<f64>,
    #[serde(default)]
    pub(crate) max_version: Option<String>,
    #[serde(default)]
    pub(crate) created_at: Option<String>,
    #[serde(default)]
    pub(crate) keywords: Option<Vec<String>>,
}

impl From<CrateJson> for SubjectRecord {
    fn from(json: CrateJson) -> Self {
        let url = format!("https://crates.io/crates/{}", json.name);
        SubjectRecord {
            kind: SourceKind::Crate,
            key: json.name,
            title: json.description,
            lang: None,
            url: Some(url),
            topics: json.keywords.unwrap_or_default(),
            created_at: json.created_at,
        }
    }
}

/// Parse a crates.io list response body into subjects.
///
/// # Errors
/// Fails when the payload is not a crates.io response.
pub fn parse_crates_json(payload: &str) -> Result<Vec<SubjectRecord>> {
    let response: CratesResponse = serde_json::from_str(payload)
        .map_err(|e| crate::error::TrendError::Config(format!("invalid crates payload: {e}")))?;
    Ok(response
        .crates
        .into_iter()
        .map(SubjectRecord::from)
        .collect())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    const SAMPLE: &str = r#"{
        "crates": [
            {
                "name": "serde",
                "description": "A serialization framework",
                "downloads": 300000000,
                "recent_downloads": 12345678.0,
                "max_version": "1.0.220",
                "created_at": "2016-01-05T00:00:00Z",
                "keywords": ["serialization", "no_std"]
            },
            {
                "name": "bare-crate",
                "downloads": 99
            }
        ],
        "meta": { "total": 2 }
    }"#;

    #[test]
    fn parse_extracts_crates_with_defaults() {
        let subjects = parse_crates_json(SAMPLE).unwrap();
        assert_eq!(subjects.len(), 2);
        assert_eq!(subjects[0].kind, SourceKind::Crate);
        assert_eq!(subjects[0].key, "serde");
        assert_eq!(
            subjects[0].title.as_deref(),
            Some("A serialization framework")
        );
        assert_eq!(
            subjects[0].topics,
            vec!["serialization".to_owned(), "no_std".to_owned()]
        );
        // Missing fields default cleanly.
        assert_eq!(subjects[1].topics, Vec::<String>::new());
        assert_eq!(subjects[1].title, None);
    }

    #[test]
    fn parse_rejects_garbage() {
        assert!(parse_crates_json("<html>").is_err());
    }
}

//! Hacker News source via the Algolia HN Search API (no auth).
//!
//! `search` (relevance-ranked) restricted to stories created inside the
//! window with at least one point approximates "trending new stories";
//! velocity comes from repeated snapshots, same as every other source.

use std::time::Duration;

use fetch_kit::Client;
use serde::{Deserialize, Serialize};

use crate::clock::unix_seconds;
use crate::error::Result;
use crate::model::{Observation, SourceKind, SubjectRecord};
use crate::source::{CollectCtx, CollectReport, TrendSource};

const HN_API: &str = "https://hn.algolia.com/api/v1";
const USER_AGENT: &str = "trendkit/0.2";
const HITS_PER_PAGE: u32 = 100;
/// Politeness pacing between paged requests (API is unauthenticated).
pub const HN_PACING: Duration = Duration::from_millis(1_100);

/// Hacker News trend source.
#[derive(Debug, Clone)]
pub struct HnSource {
    client: Client,
}

impl HnSource {
    /// Build a source. No auth: the Algolia HN API is open.
    #[must_use]
    pub fn new() -> HnSource {
        HnSource {
            client: Client::builder().user_agent(USER_AGENT).build(),
        }
    }

    async fn collect_front_page(&self, ctx: &CollectCtx, now: i64) -> Result<usize> {
        let params = HnQuery {
            tags: "front_page",
            numeric_filters: String::new(),
            hits_per_page: 30,
            page: 0,
        };
        self.collect_page(&params, ctx, now).await
    }

    async fn collect_trending(&self, ctx: &CollectCtx, now: i64) -> Result<usize> {
        let since = unix_seconds(ctx.clock.as_ref()) - i64::from(ctx.window_days) * 86_400;
        let mut total = 0usize;
        for page in 0..ctx.pages {
            let params = HnQuery {
                tags: "story",
                numeric_filters: format!("created_at_i>{since},points>0"),
                hits_per_page: HITS_PER_PAGE,
                page,
            };
            total += self.collect_page(&params, ctx, now).await?;
            if total < HITS_PER_PAGE as usize * (page as usize + 1) {
                break; // short page: last one
            }
            tokio::time::sleep(HN_PACING).await;
        }
        Ok(total)
    }

    async fn collect_page(
        &self,
        params: &HnQuery<'_>,
        ctx: &CollectCtx,
        now: i64,
    ) -> Result<usize> {
        let url = format!("{HN_API}/search");
        let response: HnResponse = self.client.get(url).query(params).json_response().await?;
        let mut count = 0usize;
        for hit in response.hits {
            let observation = Observation {
                ts: now,
                score: hit.points,
                extra1: hit.num_comments.unwrap_or(0),
                extra2: 0,
            };
            let subject = SubjectRecord::from(hit);
            let subject_id = ctx.store.upsert_subject(&subject)?;
            ctx.store.insert_observation(subject_id, &observation)?;
            count += 1;
        }
        Ok(count)
    }
}

impl Default for HnSource {
    fn default() -> Self {
        HnSource::new()
    }
}

impl TrendSource for HnSource {
    fn name(&self) -> &'static str {
        "hn"
    }

    fn collect(
        &self,
        ctx: &CollectCtx,
    ) -> impl std::future::Future<Output = Result<CollectReport>> {
        let ctx = ctx.clone();
        async move {
            let now = unix_seconds(ctx.clock.as_ref());
            let mut items = self.collect_trending(&ctx, now).await?;
            items += self.collect_front_page(&ctx, now).await?;
            ctx.store
                .record_run(now, self.name(), i64::from(ctx.pages), items as i64)?;
            Ok(CollectReport {
                source: self.name(),
                items,
                pages: ctx.pages as usize,
            })
        }
    }
}

#[derive(Debug, Serialize)]
struct HnQuery<'a> {
    tags: &'a str,
    #[serde(rename = "numericFilters")]
    numeric_filters: String,
    #[serde(rename = "hitsPerPage")]
    hits_per_page: u32,
    page: u32,
}

#[derive(Debug, Deserialize)]
struct HnResponse {
    #[serde(default)]
    hits: Vec<HnHit>,
}

/// The subset of an Algolia HN hit trendkit consumes.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct HnHit {
    #[serde(rename = "objectID", default)]
    pub(crate) object_id: String,
    #[serde(default)]
    pub(crate) title: Option<String>,
    #[serde(default)]
    pub(crate) url: Option<String>,
    #[serde(default)]
    pub(crate) points: i64,
    #[serde(rename = "num_comments", default)]
    pub(crate) num_comments: Option<i64>,
}

impl From<HnHit> for SubjectRecord {
    fn from(hit: HnHit) -> Self {
        let item_url = format!("https://news.ycombinator.com/item?id={}", hit.object_id);
        SubjectRecord {
            kind: SourceKind::HnStory,
            key: hit.object_id,
            title: hit.title,
            lang: None,
            url: Some(hit.url.unwrap_or(item_url)),
            topics: Vec::new(),
            created_at: None,
        }
    }
}

/// Parse an Algolia HN search response body into subjects.
///
/// # Errors
/// Fails when the payload is not an Algolia response.
pub fn parse_hits_json(payload: &str) -> Result<Vec<SubjectRecord>> {
    let response: HnResponse = serde_json::from_str(payload)
        .map_err(|e| crate::error::TrendError::Config(format!("invalid hn payload: {e}")))?;
    Ok(response.hits.into_iter().map(SubjectRecord::from).collect())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    const SAMPLE: &str = r#"{
        "hits": [
            {
                "objectID": "45000123",
                "title": "Show HN: A Rust crawler",
                "url": "https://github.com/example/crawler",
                "points": 156,
                "num_comments": 84,
                "author": "someone",
                "created_at_i": 1792200000
            },
            {
                "objectID": "45000999",
                "title": "Ask HN: advice?",
                "points": 3
            }
        ],
        "page": 0
    }"#;

    #[test]
    fn parse_extracts_stories_with_fallback_url() {
        let subjects = parse_hits_json(SAMPLE).unwrap();
        assert_eq!(subjects.len(), 2);
        assert_eq!(subjects[0].kind, SourceKind::HnStory);
        assert_eq!(subjects[0].key, "45000123");
        assert_eq!(
            subjects[0].url.as_deref(),
            Some("https://github.com/example/crawler")
        );
        // No external url -> HN item url.
        assert_eq!(
            subjects[1].url.as_deref(),
            Some("https://news.ycombinator.com/item?id=45000999")
        );
    }

    #[test]
    fn parse_rejects_garbage() {
        assert!(parse_hits_json("nope").is_err());
    }
}

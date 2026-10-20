//! HuggingFace trending-models source (public Hub API, no auth).
//!
//! The AI wave deserves a dedicated signal: trending *models* lead the
//! repos and stories that follow them. Score = downloads (cumulative),
//! `extra1` = likes; HF tags flow into subjects.topics, so the personal
//! profile scores model trends the same way as repo topics.

use std::time::Duration;

use fetch_kit::Client;
use serde::{Deserialize, Serialize};

use crate::clock::unix_seconds;
use crate::error::Result;
use crate::model::{Observation, SourceKind, SubjectRecord};
use crate::source::{CollectCtx, CollectReport, TrendSource};

const HF_API: &str = "https://huggingface.co/api/models";
const USER_AGENT: &str = "trendkit/0.3";
const LIMIT: u32 = 100;
/// Politeness pacing; single request per run today, kept for pagination.
pub const HF_PACING: Duration = Duration::from_millis(1_100);

/// HuggingFace trending-models source.
#[derive(Debug, Clone)]
pub struct HfSource {
    client: Client,
}

impl HfSource {
    /// Build a source.
    #[must_use]
    pub fn new() -> HfSource {
        HfSource {
            client: Client::builder().user_agent(USER_AGENT).build(),
        }
    }
}

impl Default for HfSource {
    fn default() -> Self {
        HfSource::new()
    }
}

impl TrendSource for HfSource {
    fn name(&self) -> &'static str {
        "huggingface"
    }

    fn collect(
        &self,
        ctx: &CollectCtx,
    ) -> impl std::future::Future<Output = Result<CollectReport>> {
        let ctx = ctx.clone();
        async move {
            let now = unix_seconds(ctx.clock.as_ref());
            let params = HfQuery {
                sort: "trendingScore",
                direction: -1,
                limit: LIMIT,
            };
            let response: Vec<HfModelJson> = self
                .client
                .get(HF_API)
                .query(&params)
                .json_response()
                .await?;
            let count = response.len();
            for model in response {
                let observation = Observation {
                    ts: now,
                    score: model.downloads,
                    extra1: model.likes,
                    extra2: model.trending_score.map(|f| f as i64).unwrap_or(0),
                };
                let subject = SubjectRecord::from(model);
                let subject_id = ctx.store.upsert_subject(&subject)?;
                ctx.store.insert_observation(subject_id, &observation)?;
            }
            ctx.store.record_run(now, self.name(), 1, count as i64)?;
            Ok(CollectReport {
                source: self.name(),
                items: count,
                pages: 1,
            })
        }
    }
}

#[derive(Debug, Serialize)]
struct HfQuery<'a> {
    sort: &'a str,
    direction: i32,
    limit: u32,
}

/// The subset of a Hub model entry trendkit consumes.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct HfModelJson {
    #[serde(rename = "modelId", default)]
    pub(crate) model_id: Option<String>,
    #[serde(rename = "id", default)]
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) downloads: i64,
    #[serde(default)]
    pub(crate) likes: i64,
    #[serde(default)]
    pub(crate) trending_score: Option<f64>,
    #[serde(default)]
    pub(crate) pipeline_tag: Option<String>,
    #[serde(default)]
    pub(crate) tags: Option<Vec<String>>,
}

impl From<HfModelJson> for SubjectRecord {
    fn from(json: HfModelJson) -> Self {
        let name = json.model_id.unwrap_or(json.id);
        // Hub tags carry non-topical metadata (region:us, license:other,
        // arxiv:1234.5678) that swamps topic aggregation — keep the
        // pipeline tag and the real taxonomy tags only.
        let mut topics: Vec<String> = json
            .tags
            .unwrap_or_default()
            .into_iter()
            .filter(|t| {
                !t.starts_with("region:")
                    && !t.starts_with("license:")
                    && !t.starts_with("arxiv:")
                    && !t.starts_with("doi:")
            })
            .collect();
        if let Some(pipeline) = json.pipeline_tag {
            if !topics.contains(&pipeline) {
                topics.push(pipeline);
            }
        }
        SubjectRecord {
            kind: SourceKind::HfModel,
            key: name.clone(),
            title: None,
            lang: None,
            url: Some(format!("https://huggingface.co/{name}")),
            topics,
            created_at: None,
        }
    }
}

/// Parse a Hub models response body into subjects.
///
/// # Errors
/// Fails when the payload is not a Hub API response.
pub fn parse_models_json(payload: &str) -> Result<Vec<SubjectRecord>> {
    let models: Vec<HfModelJson> = serde_json::from_str(payload)
        .map_err(|e| crate::error::TrendError::Config(format!("invalid hf payload: {e}")))?;
    Ok(models.into_iter().map(SubjectRecord::from).collect())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    const SAMPLE: &str = r#"[
        {
            "_id": "66f0a1",
            "id": "org/model-a",
            "modelId": "org/model-a",
            "downloads": 1000000,
            "likes": 4200,
            "trendingScore": 97.5,
            "pipeline_tag": "text-generation",
            "tags": ["text-generation", "region:us", "license:other", "conversational"]
        },
        {
            "id": "bare-model",
            "downloads": 42
        }
    ]"#;

    #[test]
    fn parse_extracts_models_with_pipeline_as_topic() {
        let subjects = parse_models_json(SAMPLE).unwrap();
        assert_eq!(subjects.len(), 2);
        assert_eq!(subjects[0].kind, SourceKind::HfModel);
        assert_eq!(subjects[0].key, "org/model-a");
        assert_eq!(
            subjects[0].url.as_deref(),
            Some("https://huggingface.co/org/model-a")
        );
        assert!(subjects[0].topics.contains(&"text-generation".to_owned()));
        assert!(subjects[0].topics.contains(&"conversational".to_owned()));
        // Degenerate entry falls back to `id`.
        assert_eq!(subjects[1].key, "bare-model");
        // Metadata tags (region:/license:/arxiv:) are filtered out.
        assert!(!subjects[0].topics.iter().any(|t| t.contains(':')));
    }

    #[test]
    fn parse_rejects_garbage() {
        assert!(parse_models_json("{").is_err());
    }
}

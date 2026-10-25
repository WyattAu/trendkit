//! SEC EDGAR source via full-text search (no auth, fair-access pacing).
//!
//! Tracks M&A signal volume per filer: each pass runs a small set of
//! curated form-scoped queries over a rolling window and records, per
//! CIK, the number of distinct matching filings currently inside the
//! window. The score is recomputed fresh each pass, so repeated runs
//! never inflate it; its day-over-day delta is filings velocity, the
//! same machinery every other source feeds.

use std::collections::HashMap;
use std::time::Duration;

use bizkit_core::edgar::{EdgarClient, FtsQuery};

use crate::clock::{iso_date_from_unix, unix_seconds};
use crate::error::{Result, TrendError};
use crate::model::{Observation, SourceKind, SubjectRecord};
use crate::source::{CollectCtx, CollectReport, TrendSource};

/// Politeness pacing between EDGAR queries (SEC fair-access policy).
pub const EDGAR_PACING: Duration = Duration::from_millis(400);

/// One curated M&A watch query: quoted phrase plus form filter.
struct WatchQuery {
    phrase: &'static str,
    forms: &'static str,
}

/// The v1 M&A watchlist. Three queries per pass keeps the pass inside
/// SEC fair-access limits; widen cautiously. `acquisition of` was
/// bizkit's highest-yield query (100 hits in the Oct 24 baseline
/// crawl) and is kept so the daily series stays comparable to it.
const WATCHLIST: [WatchQuery; 3] = [
    WatchQuery {
        phrase: "\"merger agreement\"",
        forms: "8-K,DEFM14A,425,S-4",
    },
    WatchQuery {
        phrase: "\"acquisition of\"",
        forms: "8-K,DEFM14A,425,S-4,SC 13D,SC 13G",
    },
    WatchQuery {
        phrase: "\"tender offer\"",
        forms: "SC TO-T,SC 14D9,425,8-K",
    },
];

/// SEC EDGAR trend source.
#[derive(Debug, Clone)]
pub struct EdgarSource {
    client: EdgarClient,
}

impl EdgarSource {
    /// Build a source. No auth: EDGAR full-text search is open.
    #[must_use]
    pub fn new() -> EdgarSource {
        EdgarSource {
            client: EdgarClient::new(),
        }
    }
}

impl Default for EdgarSource {
    fn default() -> Self {
        EdgarSource::new()
    }
}

impl TrendSource for EdgarSource {
    fn name(&self) -> &'static str {
        "edgar"
    }

    fn collect(
        &self,
        ctx: &CollectCtx,
    ) -> impl std::future::Future<Output = Result<CollectReport>> {
        let ctx = ctx.clone();
        async move {
            let now = unix_seconds(ctx.clock.as_ref());
            let today = iso_date_from_unix(now);
            let start = iso_date_from_unix(now - i64::from(ctx.window_days) * 86_400);

            // Filing identity across the whole pass: a filing matched
            // by two watch queries counts once.
            let mut seen: HashMap<(String, String), ()> = HashMap::new();
            let mut names: HashMap<String, String> = HashMap::new();
            let mut items = 0usize;
            for (i, w) in WATCHLIST.iter().enumerate() {
                let query = FtsQuery {
                    q: w.phrase.to_string(),
                    forms: Some(w.forms.to_string()),
                    startdt: start.clone(),
                    enddt: today.clone(),
                };
                let hits = match self.client.search(&query, 100).await {
                    Ok(h) => h,
                    // A failing watch query must not sink the pass.
                    Err(err) => {
                        if i + 1 == WATCHLIST.len() && seen.is_empty() {
                            return Err(TrendError::Other(format!("edgar search failed: {err}")));
                        }
                        tokio::time::sleep(EDGAR_PACING).await;
                        continue;
                    }
                };
                for hit in hits {
                    items += 1;
                    seen.entry((hit.cik.clone(), hit.doc.clone())).or_insert(());
                    names.insert(hit.cik.clone(), hit.display_name.clone());
                }
                tokio::time::sleep(EDGAR_PACING).await;
            }

            // Per-filer window count, written as one observation each.
            let mut counts: HashMap<String, i64> = HashMap::new();
            for (cik, _doc) in seen.keys() {
                *counts.entry(cik.clone()).or_default() += 1;
            }
            for (cik, score) in counts {
                let subject = SubjectRecord {
                    kind: SourceKind::Edgar,
                    key: cik.clone(),
                    title: names.get(&cik).cloned(),
                    lang: None,
                    url: Some(format!(
                        "https://www.sec.gov/cgi-bin/browse-edgar?action=getcompany&CIK={cik}"
                    )),
                    topics: vec!["edgar".to_owned(), "mna".to_owned()],
                    created_at: None,
                };
                let subject_id = ctx.store.upsert_subject(&subject)?;
                ctx.store.insert_observation(
                    subject_id,
                    &Observation {
                        ts: now,
                        score,
                        extra1: 0,
                        extra2: 0,
                    },
                )?;
            }

            ctx.store.record_run(
                now,
                self.name(),
                i64::from(WATCHLIST.len() as u32),
                items as i64,
            )?;
            Ok(CollectReport {
                source: self.name(),
                items,
                pages: WATCHLIST.len(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    /// The source identifies as `edgar` so collect_runs rows line up.
    #[test]
    fn name_is_edgar() {
        assert_eq!(EdgarSource::new().name(), "edgar");
    }

    /// Window dates derive from the clock, not the wall: the ISO
    /// helper renders a fixed instant and 7 days earlier as distinct
    /// `YYYY-MM-DD` strings.
    #[test]
    fn window_dates_follow_clock() {
        let now = iso_date_from_unix(1_792_944_000); // 2026-10-25-ish
        let start = iso_date_from_unix(1_792_944_000 - 7 * 86_400);
        assert_eq!(now.len(), 10);
        assert_eq!(start.len(), 10);
        assert_ne!(now, start);
    }
}

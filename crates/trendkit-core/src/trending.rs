//! GitHub trending-page source: the one source with no API.
//!
//! `github.com/trending` exposes no JSON endpoint, so the daily page is
//! fetched and parsed as HTML (via `scraper`/html5ever — no regex
//! scraping). Subjects use `SourceKind::GithubRepo` with `owner/repo`
//! keys, so trending-page observations land in the *same subject space*
//! as Search-API observations: two independent signals per repository.
//! `extra1` carries "stars today" — a second velocity signal GitHub
//! gives us for free.
//!
//! Markup drift is handled by optionality: every field parses to
//! `Option`/0 and articles without a repo path are skipped rather than
//! guessed at.

use fetch_kit::Client;
use scraper::{ElementRef, Html, Selector};

use crate::clock::unix_seconds;
use crate::error::{Result, TrendError};
use crate::model::{Observation, SourceKind, SubjectRecord};
use crate::source::{CollectCtx, CollectReport, TrendSource};

const TRENDING_URL: &str = "https://github.com/trending?since=daily";
const USER_AGENT: &str = "trendkit/0.2";

/// One parsed row of the trending page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrendingRow {
    /// `owner/repo` path from the article link (leading slash stripped).
    pub full_name: String,
    /// Article description, if present.
    pub description: Option<String>,
    /// Primary language, if present.
    pub language: Option<String>,
    /// Total stars.
    pub stars: i64,
    /// Total forks.
    pub forks: i64,
    /// "N stars today" from the article footer.
    pub stars_today: i64,
}

/// GitHub trending-page source.
#[derive(Debug, Clone)]
pub struct TrendingSource {
    client: Client,
}

impl TrendingSource {
    /// Build a source.
    #[must_use]
    pub fn new() -> TrendingSource {
        TrendingSource {
            client: Client::builder().user_agent(USER_AGENT).build(),
        }
    }

    /// Fetch the trending page body.
    ///
    /// # Errors
    /// Fails on fetch errors (status codes checked by fetch-kit).
    pub async fn fetch_html(&self) -> Result<String> {
        let response = self.client.get(TRENDING_URL).send().await?;
        Ok(response.text().await.unwrap_or_default())
    }
}

impl Default for TrendingSource {
    fn default() -> Self {
        TrendingSource::new()
    }
}

impl TrendSource for TrendingSource {
    fn name(&self) -> &'static str {
        "github_trending"
    }

    fn collect(
        &self,
        ctx: &CollectCtx,
    ) -> impl std::future::Future<Output = Result<CollectReport>> {
        let ctx = ctx.clone();
        async move {
            let now = unix_seconds(ctx.clock.as_ref());
            let html = self.fetch_html().await?;
            let rows = parse_trending_html(&html)?;
            let count = rows.len();
            for row in rows {
                let observation = Observation {
                    ts: now,
                    score: row.stars,
                    extra1: row.stars_today,
                    extra2: row.forks,
                };
                let subject = SubjectRecord {
                    kind: SourceKind::GithubRepo,
                    key: row.full_name,
                    title: row.description,
                    lang: row.language,
                    url: None, // resolved by the GitHub source's subjects
                    topics: Vec::new(),
                    created_at: None,
                };
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

fn selector(css: &str) -> Result<Selector> {
    Selector::parse(css).map_err(|e| TrendError::Config(format!("bad selector `{css}`: {e}")))
}

/// "1,234" / "1,234 stars today" / "star today" → i64 (0 when absent).
fn parse_count(text: &str, strip_suffix_words: bool) -> i64 {
    let cleaned = if strip_suffix_words {
        text.split_whitespace().next().unwrap_or("")
    } else {
        text.trim()
    };
    cleaned.replace(',', "").parse::<i64>().unwrap_or(0)
}

/// Trimmed concatenated text of an element.
fn text_of(el: &ElementRef<'_>) -> String {
    el.text().collect::<String>().trim().to_owned()
}

/// Parse the GitHub trending page HTML into rows.
///
/// # Errors
/// Fails only on internal selector construction; rows with missing
/// article data are skipped, and a page with no articles yields no rows.
pub fn parse_trending_html(html: &str) -> Result<Vec<TrendingRow>> {
    let document = Html::parse_document(html);
    let articles = selector("article.Box-row")?;
    let name_link = selector("h2 a")?;
    let description = selector("p")?;
    let language = selector("span[itemprop=\"programmingLanguage\"]")?;
    let counters = selector("a.Link--muted")?;
    let stars_today = selector("span.d-inline-block.float-sm-right")?;

    let mut rows = Vec::new();
    for article in document.select(&articles) {
        let Some(link) = article.select(&name_link).next() else {
            continue;
        };
        let Some(href) = link.value().attr("href") else {
            continue;
        };
        let full_name = href.trim_start_matches('/').to_owned();
        if full_name.is_empty() || !full_name.contains('/') {
            continue;
        }

        let stars_and_forks: Vec<i64> = article
            .select(&counters)
            .map(|c| parse_count(&text_of(&c), false))
            .collect();
        // Markup order is stars then forks; missing counters read as 0.
        let stars = stars_and_forks.first().copied().unwrap_or(0);
        let forks = stars_and_forks.get(1).copied().unwrap_or(0);

        rows.push(TrendingRow {
            full_name,
            description: article.select(&description).next().map(|p| text_of(&p)),
            language: article.select(&language).next().map(|l| text_of(&l)),
            stars,
            forks,
            stars_today: article
                .select(&stars_today)
                .next()
                .map_or(0, |s| parse_count(&text_of(&s), true)),
        });
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    const SAMPLE: &str = r#"
    <main>
      <article class="Box-row">
        <h2 class="h3 lh-condensed"><a href="/owner/first-repo">first-repo</a></h2>
        <p class="col-9 color-fg-muted my-1 pr-4">A trending thing</p>
        <span itemprop="programmingLanguage">Rust</span>
        <a class="Link--muted" href="/owner/first-repo/stargazers">4,149</a>
        <a class="Link--muted" href="/owner/first-repo/forks">321</a>
        <span class="d-inline-block float-sm-right">1,234 stars today</span>
      </article>
      <article class="Box-row">
        <h2 class="h3 lh-condensed"><a href="/owner/second-repo">second-repo</a></h2>
        <a class="Link--muted" href="/owner/second-repo/stargazers">12</a>
        <span class="d-inline-block float-sm-right">3 stars today</span>
      </article>
      <article class="Box-row">
        <h2 class="h3 lh-condensed"></h2>
      </article>
    </main>"#;

    #[test]
    fn parses_rows_with_defaults_and_skips_incomplete() {
        let rows = parse_trending_html(SAMPLE).unwrap();
        assert_eq!(rows.len(), 2); // third article has no repo link

        assert_eq!(rows[0].full_name, "owner/first-repo");
        assert_eq!(rows[0].description.as_deref(), Some("A trending thing"));
        assert_eq!(rows[0].language.as_deref(), Some("Rust"));
        assert_eq!(rows[0].stars, 4_149);
        assert_eq!(rows[0].forks, 321);
        assert_eq!(rows[0].stars_today, 1_234);

        assert_eq!(rows[1].full_name, "owner/second-repo");
        assert_eq!(rows[1].description, None);
        assert_eq!(rows[1].language, None);
        assert_eq!(rows[1].stars, 12);
        assert_eq!(rows[1].forks, 0);
        assert_eq!(rows[1].stars_today, 3);
    }

    #[test]
    fn empty_page_yields_no_rows() {
        assert!(parse_trending_html("<html><body>sign in</body></html>")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn count_parser_handles_noise() {
        assert_eq!(parse_count("4,149", false), 4_149);
        assert_eq!(parse_count("1,234 stars today", true), 1_234);
        assert_eq!(parse_count("1 star today", true), 1);
        assert_eq!(parse_count("", false), 0);
        assert_eq!(parse_count("n/a", false), 0);
    }
}

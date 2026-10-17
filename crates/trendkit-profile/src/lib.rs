//! Personal relevance profile for trendkit.
//!
//! Scans a GitHub user's own repositories (forks excluded) and builds
//! language/topic histograms. Trending repos are then ranked by
//! attributable overlap — shared topics and matching primary language —
//! plus the global velocity, via
//! [`trendkit_core::score::personal_score`].
//!
//! This is the day-one-useful half of trendkit: it needs no snapshot
//! history, only one API listing.

use std::collections::BTreeMap;

use trendkit_core::error::Result;
use trendkit_core::github::GithubSource;
use trendkit_core::model::RepoListed;
use trendkit_core::score::personal_score;
use trendkit_core::store::Store;

/// Aggregated interest profile built from a user's repositories.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Profile {
    /// Language → count across the user's non-fork repos.
    pub languages: BTreeMap<String, usize>,
    /// Topic → count across the user's non-fork repos.
    pub topics: BTreeMap<String, usize>,
    /// How many non-fork repos the profile was built from.
    pub repo_count: usize,
}

impl Profile {
    /// True when the profile has no repositories (nothing to score with).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.repo_count == 0
    }

    /// Build a profile from a user's repo listing; forks are ignored.
    #[must_use]
    pub fn from_repos(rows: &[RepoListed]) -> Profile {
        let mut profile = Profile::default();
        for row in rows {
            if row.is_fork {
                continue;
            }
            profile.repo_count += 1;
            if let Some(language) = &row.language {
                *profile.languages.entry(language.clone()).or_insert(0) += 1;
            }
            for topic in &row.topics {
                *profile.topics.entry(topic.clone()).or_insert(0) += 1;
            }
        }
        profile
    }

    /// Top `n` languages by count, ties broken alphabetically.
    #[must_use]
    pub fn top_languages(&self, n: usize) -> Vec<(String, usize)> {
        top_of(&self.languages, n)
    }

    /// Top `n` topics by count, ties broken alphabetically.
    #[must_use]
    pub fn top_topics(&self, n: usize) -> Vec<(String, usize)> {
        top_of(&self.topics, n)
    }

    /// How many of `topics` appear in this profile.
    #[must_use]
    pub fn topic_overlap(&self, topics: &[String]) -> usize {
        topics
            .iter()
            .filter(|t| self.topics.contains_key(*t))
            .count()
    }

    /// Whether `language` is one of the user's languages.
    #[must_use]
    pub fn language_match(&self, language: Option<&str>) -> bool {
        language.is_some_and(|l| self.languages.contains_key(l))
    }

    /// Personal relevance score for one trending repo (see `score`).
    #[must_use]
    pub fn score(&self, velocity_per_day: f64, language: Option<&str>, topics: &[String]) -> f64 {
        personal_score(
            velocity_per_day,
            self.topic_overlap(topics),
            self.language_match(language),
        )
    }
}

fn top_of(map: &BTreeMap<String, usize>, n: usize) -> Vec<(String, usize)> {
    let mut items: Vec<(String, usize)> = map.iter().map(|(k, v)| (k.clone(), *v)).collect();
    items.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    items.truncate(n);
    items
}

/// Fetch the user's repos, persist them as the profile, and return the
/// aggregated [`Profile`].
///
/// # Errors
/// Fails on API or store errors; an empty listing is not an error.
pub async fn update_profile(source: &GithubSource, owner: &str, store: &Store) -> Result<Profile> {
    let repos = source.list_user_repos(owner).await?;
    store.save_profile_rows(&repos)?;
    Ok(Profile::from_repos(&repos))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn repo(name: &str, lang: Option<&str>, topics: &[&str], fork: bool) -> RepoListed {
        RepoListed {
            full_name: name.to_owned(),
            language: lang.map(str::to_owned),
            topics: topics.iter().map(|t| (*t).to_owned()).collect(),
            description: None,
            is_fork: fork,
            pushed_at: None,
            stars: 1,
        }
    }

    #[test]
    fn forks_are_excluded() {
        let rows = vec![
            repo("a/own", Some("Rust"), &["cli"], false),
            repo("b/fork", Some("Rust"), &["cli"], true),
        ];
        let profile = Profile::from_repos(&rows);
        assert_eq!(profile.repo_count, 1);
        assert_eq!(profile.languages.get("Rust"), Some(&1));
    }

    #[test]
    fn overlap_counts_shared_topics() {
        let rows = vec![repo("a/x", Some("Rust"), &["cli", "wasm", "parser"], false)];
        let profile = Profile::from_repos(&rows);
        let candidates = vec!["wasm".to_owned(), "cli".to_owned(), "unrelated".to_owned()];
        assert_eq!(profile.topic_overlap(&candidates), 2);
    }

    #[test]
    fn score_rewards_relevance_over_raw_velocity() {
        let rows = vec![repo("a/x", Some("Rust"), &["cli", "wasm"], false)];
        let profile = Profile::from_repos(&rows);

        let relevant = profile.score(1.0, Some("Rust"), &["cli".to_owned(), "wasm".to_owned()]);
        let irrelevant = profile.score(5.0, Some("Java"), &["kubernetes".to_owned()]);
        assert!(
            relevant > irrelevant,
            "relevance must beat a modest velocity gap: {relevant} vs {irrelevant}"
        );
    }

    #[test]
    fn top_lists_sort_by_count_then_name() {
        let rows = vec![
            repo("a/x", Some("Rust"), &["cli"], false),
            repo("b/y", Some("Rust"), &["cli"], false),
            repo("c/z", Some("Go"), &["wasm"], false),
        ];
        let profile = Profile::from_repos(&rows);
        assert_eq!(
            profile.top_languages(5),
            vec![("Rust".to_owned(), 2), ("Go".to_owned(), 1)]
        );
        assert_eq!(profile.top_topics(1), vec![("cli".to_owned(), 2)]);
    }
}

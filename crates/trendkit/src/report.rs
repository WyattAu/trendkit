//! Report rendering: plain table and markdown, with optional personal
//! relevance ranking. Rows mix kinds; each row is tagged (GH/HN/CR) and
//! linked per kind.

use trendkit_core::model::VelocityRow;
use trendkit_profile::Profile;

/// A row ready for rendering: trend data plus its optional personal score.
pub struct ReportRow<'a> {
    /// Underlying velocity row.
    pub row: &'a VelocityRow,
    /// Personal relevance score, when a profile is in play.
    pub score: Option<f64>,
}

/// Compose report rows; when `profile` is present, rows are re-sorted by
/// personal score (descending), otherwise the velocity order is kept.
#[must_use]
pub fn build_rows<'a>(rows: &'a [VelocityRow], profile: Option<&Profile>) -> Vec<ReportRow<'a>> {
    let mut out: Vec<ReportRow<'a>> = rows
        .iter()
        .map(|row| ReportRow {
            row,
            score: profile.map(|p| p.score(row.velocity_per_day, row.lang.as_deref(), &row.topics)),
        })
        .collect();
    if profile.is_some() {
        out.sort_by(|a, b| {
            let sa = a.score.unwrap_or(f64::NEG_INFINITY);
            let sb = b.score.unwrap_or(f64::NEG_INFINITY);
            sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
        });
    }
    out
}

/// Compose the header block shown above every report.
#[must_use]
pub fn header_lines(
    window_days: u32,
    db_display: &str,
    subjects: i64,
    observations: i64,
    last_run: Option<(i64, &str, i64)>,
    profile_line: Option<String>,
) -> Vec<String> {
    let mut header = vec![format!(
        "trendkit report — window {window_days}d, db {db_display} ({subjects} subjects, {observations} observations)"
    )];
    match last_run {
        Some((ts, source, items)) => header.push(format!(
            "last collection: {source} wrote {items} observations at unix {ts}"
        )),
        None => header.push("last collection: never".to_owned()),
    }
    if let Some(line) = profile_line {
        header.push(line);
    }
    header.push(
        "score = stars (GH) / points (HN) / downloads (CR); velocity = Δscore/day".to_owned(),
    );
    header
}

/// Render the report as plain text.
#[must_use]
pub fn render_plain(rows: &[ReportRow<'_>], header: &[String]) -> String {
    let mut out = header.join("\n");
    out.push('\n');
    if rows.is_empty() {
        out.push_str(
            "\nNo snapshot history yet — run `trendkit collect` (velocity needs >= 2 snapshots).\n",
        );
        return out;
    }

    let key_width = rows
        .iter()
        .map(|r| r.row.key.len())
        .max()
        .unwrap_or(0)
        .max(4);
    let lang_width = rows
        .iter()
        .map(|r| r.row.lang.as_deref().map_or(0, str::len))
        .max()
        .unwrap_or(0)
        .max(4);
    let score_col = rows.first().is_some_and(|r| r.score.is_some());

    let mut table = String::new();
    if score_col {
        table.push_str(&format!(
            "{:<2}  {:<kw$}  {:<lw$}  {:>9}  {:>10}  {:>8}\n",
            "K",
            "SUBJECT",
            "LANG",
            "SCORE",
            "VEL/DAY",
            "PERSONA",
            kw = key_width,
            lw = lang_width,
        ));
    } else {
        table.push_str(&format!(
            "{:<2}  {:<kw$}  {:<lw$}  {:>9}  {:>10}\n",
            "K",
            "SUBJECT",
            "LANG",
            "SCORE",
            "VEL/DAY",
            kw = key_width,
            lw = lang_width,
        ));
    }

    for r in rows {
        let lang = r.row.lang.as_deref().unwrap_or("-");
        if let Some(score) = r.score {
            table.push_str(&format!(
                "{:<2}  {:<kw$}  {:<lw$}  {:>9}  {:>10.1}  {:>8.1}\n",
                r.row.kind.tag(),
                r.row.key,
                lang,
                r.row.score_now,
                r.row.velocity_per_day,
                score,
                kw = key_width,
                lw = lang_width,
            ));
        } else {
            table.push_str(&format!(
                "{:<2}  {:<kw$}  {:<lw$}  {:>9}  {:>10.1}\n",
                r.row.kind.tag(),
                r.row.key,
                lang,
                r.row.score_now,
                r.row.velocity_per_day,
                kw = key_width,
                lw = lang_width,
            ));
        }
    }
    out.push_str(&table);
    out
}

/// Render the report as a markdown table (repo/story/crate links included).
#[must_use]
pub fn render_markdown(rows: &[ReportRow<'_>], header: &[String]) -> String {
    let mut out = String::new();
    for line in header {
        out.push_str(line);
        out.push('\n');
    }
    if rows.is_empty() {
        out.push_str("\n_No snapshot history yet — run `trendkit collect` (velocity needs >= 2 snapshots)._\n");
        return out;
    }

    out.push_str("\n| K | Subject | Lang | Score | V/day |");
    if rows.first().is_some_and(|r| r.score.is_some()) {
        out.push_str(" Score |");
    }
    out.push_str("\n|---|---|---|---:|---:|");
    if rows.first().is_some_and(|r| r.score.is_some()) {
        out.push_str("---:|");
    }
    out.push('\n');

    for r in rows {
        let lang = r.row.lang.as_deref().unwrap_or("-");
        let link = match &r.row.url {
            Some(url) => format!("[{}]({})", r.row.key.replace('|', "/"), url),
            None => r.row.key.clone(),
        };
        let title = r.row.title.as_deref().map_or(String::new(), |t| {
            // Newlines inside descriptions split the table row.
            format!(" — {}", t.replace('|', "/").replace(['\r', '\n'], " "))
        });
        out.push_str(&format!(
            "| {} | {}{} | {} | {} | {:.1} |",
            r.row.kind.tag(),
            link,
            title,
            lang,
            r.row.score_now,
            r.row.velocity_per_day,
        ));
        if let Some(score) = r.score {
            out.push_str(&format!(" {:.1} |", score));
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use trendkit_core::model::SourceKind;

    fn row(kind: SourceKind, key: &str, lang: Option<&str>, velocity: f64) -> VelocityRow {
        VelocityRow {
            kind,
            key: key.to_owned(),
            title: Some("desc | with pipe".to_owned()),
            lang: lang.map(str::to_owned),
            url: Some(format!("https://example.com/{key}")),
            topics: vec!["cli".to_owned()],
            score_now: 100,
            score_prev: 50,
            window_secs: 86_400,
            velocity_per_day: velocity,
        }
    }

    #[test]
    fn plain_report_lists_velocity_order() {
        let rows = vec![
            row(SourceKind::GithubRepo, "a/one", Some("Rust"), 70.0),
            row(SourceKind::Crate, "serde", None, 5.0),
        ];
        let built = build_rows(&rows, None);
        let text = render_plain(&built, &["header".to_owned()]);
        assert!(text.starts_with("header\n"));
        assert!(text.contains("a/one"));
        assert!(text.contains("70.0"));
        assert!(text.contains("GH"));
        assert!(text.contains("CR"));
        assert!(!text.contains("PERSONA"));
    }

    #[test]
    fn personal_report_resorts_by_score() {
        let rows = vec![
            row(SourceKind::GithubRepo, "a/irrelevant", Some("Java"), 100.0),
            row(SourceKind::GithubRepo, "b/relevant", Some("Rust"), 1.0),
        ];
        let profile_rows = vec![trendkit_core::model::RepoListed {
            full_name: "x/rust-repo".to_owned(),
            language: Some("Rust".to_owned()),
            topics: vec!["cli".to_owned()],
            description: None,
            is_fork: false,
            pushed_at: None,
            stars: 1,
        }];
        let profile = Profile::from_repos(&profile_rows);
        let built = build_rows(&rows, Some(&profile));
        assert_eq!(built[0].row.key, "b/relevant");
        let text = render_plain(&built, &[]);
        assert!(text.contains("PERSONA"));
    }

    #[test]
    fn markdown_escapes_pipes_in_descriptions() {
        let mut one = row(SourceKind::GithubRepo, "a/one", Some("Rust"), 7.0);
        one.title = Some("desc | with pipe\nand newline".to_owned());
        let rows = vec![one];
        let built = build_rows(&rows, None);
        let text = render_markdown(&built, &[]);
        assert!(
            text.contains("desc / with pipe and newline"),
            "TEXT WAS: {text}"
        );
        assert!(!text.contains("desc | with"));
        assert!(!text
            .lines()
            .any(|l| !l.starts_with('|') && l.contains('|') && l.contains("a/one")));
        assert!(text.contains("https://example.com/a/one"));
    }

    #[test]
    fn markdown_with_scores_has_extra_column() {
        let rows = vec![row(SourceKind::HnStory, "42", None, 7.0)];
        let profile = Profile::default();
        let built = build_rows(&rows, Some(&profile));
        let text = render_markdown(&built, &[]);
        assert!(text.contains("Score |"));
        assert!(text.contains("| HN |"));
    }

    #[test]
    fn empty_report_explains_itself() {
        let built = build_rows(&[], None);
        let text = render_plain(&built, &[]);
        assert!(text.contains("trendkit collect"));
    }

    #[test]
    fn header_notes_score_semantics() {
        let lines = header_lines(14, "db", 1, 2, None, None);
        assert!(lines.iter().any(|l| l.contains("stars (GH)")));
    }
}

//! Report rendering: plain table and markdown, with optional personal
//! relevance ranking.

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
            score: profile
                .map(|p| p.score(row.velocity_per_day, row.language.as_deref(), &row.topics)),
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

    let name_width = rows
        .iter()
        .map(|r| r.row.full_name.len())
        .max()
        .unwrap_or(0)
        .max(4);
    let lang_width = rows
        .iter()
        .map(|r| r.row.language.as_deref().map_or(0, str::len))
        .max()
        .unwrap_or(0)
        .max(4);

    let score_col = rows.first().is_some_and(|r| r.score.is_some());
    let mut table = String::new();
    if score_col {
        table.push_str(&format!(
            "{:<width$}  {:<lw$}  {:>7}  {:>10}  {:>8}\n",
            "REPO",
            "LANG",
            "STARS",
            "VEL/DAY",
            "SCORE",
            width = name_width,
            lw = lang_width,
        ));
    } else {
        table.push_str(&format!(
            "{:<width$}  {:<lw$}  {:>7}  {:>10}\n",
            "REPO",
            "LANG",
            "STARS",
            "VEL/DAY",
            width = name_width,
            lw = lang_width,
        ));
    }

    for r in rows {
        let lang = r.row.language.as_deref().unwrap_or("-");
        let delta = r.row.stars_now - r.row.stars_prev;
        if let Some(score) = r.score {
            table.push_str(&format!(
                "{:<nw$}  {:<lw$}  {:>7}  {:>10.1}  {:>8.1}\n",
                r.row.full_name,
                lang,
                r.row.stars_now,
                r.row.velocity_per_day,
                score,
                nw = name_width,
                lw = lang_width,
            ));
        } else {
            table.push_str(&format!(
                "{:<nw$}  {:<lw$}  {:>7}  {:>+9}Δ {:>10.1}\n",
                r.row.full_name,
                lang,
                r.row.stars_now,
                delta,
                r.row.velocity_per_day,
                nw = name_width,
                lw = lang_width,
            ));
        }
    }
    out.push_str(&table);
    out
}

/// Render the report as a markdown table.
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

    out.push_str("\n| Repo | Lang | Stars | Velocity/day");
    if rows.first().is_some_and(|r| r.score.is_some()) {
        out.push_str(" | Score |");
    } else {
        out.push_str(" |");
    }
    out.push_str("\n|---|---|---:|---:|\n");
    if rows.first().is_some_and(|r| r.score.is_some()) {
        out.insert(out.len(), '\n');
    }

    for r in rows {
        let lang = r.row.language.as_deref().unwrap_or("-");
        let desc = r
            .row
            .description
            .as_deref()
            .map_or(String::new(), |d| format!(" — {}", d.replace('|', "/")));
        match r.score {
            Some(score) => out.push_str(&format!(
                "| [{}]({}){} | {} | {} | {:.1} | {:.1} |\n",
                r.row.full_name,
                html_url(&r.row.full_name),
                desc,
                lang,
                r.row.stars_now,
                r.row.velocity_per_day,
                score,
            )),
            None => out.push_str(&format!(
                "| [{}]({}){} | {} | {} | {:.1} |\n",
                r.row.full_name,
                html_url(&r.row.full_name),
                desc,
                lang,
                r.row.stars_now,
                r.row.velocity_per_day,
            )),
        }
    }
    out
}

fn html_url(full_name: &str) -> String {
    format!("https://github.com/{full_name}")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use trendkit_core::model::VelocityRow;

    fn row(name: &str, lang: Option<&str>, velocity: f64) -> VelocityRow {
        VelocityRow {
            full_name: name.to_owned(),
            language: lang.map(str::to_owned),
            description: Some("desc | with pipe".to_owned()),
            topics: vec!["cli".to_owned()],
            stars_now: 100,
            stars_prev: 50,
            window_secs: 86_400,
            velocity_per_day: velocity,
        }
    }

    #[test]
    fn plain_report_lists_velocity_order() {
        let rows = vec![row("a/one", Some("Rust"), 70.0), row("b/two", None, 5.0)];
        let built = build_rows(&rows, None);
        let text = render_plain(&built, &["header".to_owned()]);
        assert!(text.starts_with("header\n"));
        assert!(text.contains("a/one"));
        assert!(text.contains("70.0"));
        assert!(!text.contains("SCORE"));
    }

    #[test]
    fn personal_report_resorts_by_score() {
        let rows = vec![
            row("a/irrelevant", Some("Java"), 100.0),
            row("b/relevant", Some("Rust"), 1.0),
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
        assert_eq!(built[0].row.full_name, "b/relevant");
        let text = render_plain(&built, &[]);
        assert!(text.contains("SCORE"));
    }

    #[test]
    fn markdown_escapes_pipes_in_descriptions() {
        let rows = vec![row("a/one", Some("Rust"), 7.0)];
        let built = build_rows(&rows, None);
        let text = render_markdown(&built, &[]);
        assert!(text.contains("desc / with pipe"));
        assert!(!text.contains("desc | with"));
        assert!(text.contains("https://github.com/a/one"));
    }

    #[test]
    fn empty_report_explains_itself() {
        let built = build_rows(&[], None);
        let text = render_plain(&built, &[]);
        assert!(text.contains("trendkit collect"));
    }
}

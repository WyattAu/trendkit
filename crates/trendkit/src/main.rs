//! trendkit — dev-trend crawler CLI.
//!
//! Subcommands:
//! - `collect` — snapshot observations into the SQLite store
//! - `report`  — velocity ranking with optional integrity, shape,
//!   topics, persistence, AI and Telegram sections
//! - `profile` — manage the personal relevance profile

mod config;
mod report;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use clock_kit::SystemClock;
use config_kit::Sensitive;
use trendkit_core::cratesio::CratesSource;
use trendkit_core::edgar::EdgarSource;
use trendkit_core::github::GithubSource;
use trendkit_core::hf::HfSource;
use trendkit_core::hn::HnSource;
use trendkit_core::source::{CollectCtx, TrendSource};
use trendkit_core::store::Store;
use trendkit_core::trending::TrendingSource;
use trendkit_profile::{update_profile, Profile};

/// Dev-trend crawler: snapshots, velocity, personal relevance.
#[derive(Debug, Parser)]
#[command(name = "trendkit", version, about)]
struct Cli {
    /// TOML config path (default: ./trendkit.toml when present).
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Collect snapshot observations into the store.
    Collect {
        /// Which sources to run.
        #[arg(long, value_enum, default_value_t = SourceSel::All)]
        source: SourceSel,
        /// Pages per source (overrides config).
        #[arg(long)]
        pages: Option<u32>,
        /// Days back for "recent" queries (overrides config).
        #[arg(long)]
        window_days: Option<u32>,
        /// Skip the personal-profile refresh.
        #[arg(long, default_value_t = false)]
        no_profile: bool,
    },
    /// Report the highest-velocity subjects.
    Report {
        /// Rows to show.
        #[arg(long, default_value_t = 25)]
        top: u32,
        /// Velocity window in days.
        #[arg(long, default_value_t = 14)]
        window: u32,
        /// Emit markdown (with links) instead of a table.
        #[arg(long, default_value_t = false)]
        markdown: bool,
        /// Rank by personal relevance (needs a stored profile).
        #[arg(long, default_value_t = false)]
        personal: bool,
        /// Only show these kinds (comma-separated: gh, hn, cr, hf).
        #[arg(long, value_delimiter = ',')]
        kinds: Vec<String>,
        /// Only show subjects trending on >= N distinct days (0 = off).
        #[arg(long, default_value_t = 0)]
        persistent: u32,
        /// Append shape classification (accelerating/steady/fading) for
        /// subjects with >= N distinct days.
        #[arg(long, default_value_t = 0)]
        shapes: u32,
        /// Project next-day deltas for subjects with >= N daily points.
        #[arg(long, default_value_t = 0)]
        forecast: u32,
        /// Append integrity signals (fork flags, org bursts, clone
        /// farms, outliers).
        #[arg(long, default_value_t = false)]
        integrity: bool,
        /// Append the top-N trending topics across the window.
        #[arg(long, default_value_t = 0)]
        topics: u32,
        /// Prepend an AI briefing (needs ai_api_key; skips if absent).
        #[arg(long, default_value_t = false)]
        ai: bool,
        /// Also send the markdown report via Telegram.
        #[arg(long, default_value_t = false)]
        telegram: bool,
        /// Also write the rendered report to this file (markdown mode
        /// recommended) — for weekly public pages.
        #[arg(long)]
        out: Option<std::path::PathBuf>,
    },
    /// Project next-day deltas for subjects with >= N daily points.
    Forecast {
        /// Minimum daily data points to project (default 3).
        #[arg(long, default_value_t = 3)]
        min_points: u32,
        /// SQLite store path.
        #[arg(long, default_value = "trendkit.db")]
        db: PathBuf,
    },
    /// Manage the personal relevance profile.
    Profile {
        #[command(subcommand)]
        cmd: ProfileCommand,
    },
}

#[derive(Debug, Subcommand)]
enum ProfileCommand {
    /// Refresh the profile from the GitHub API.
    Update {
        /// Owner (overrides config).
        #[arg(long)]
        owner: Option<String>,
    },
    /// Show the stored profile summary.
    Show,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum SourceSel {
    All,
    Github,
    Hn,
    Cratesio,
    Trending,
    Huggingface,
    Edgar,
}

/// Flags for one `report` invocation.
#[derive(Debug, Clone, Default)]
struct ReportOpts {
    /// Rows to show.
    top: u32,
    /// Velocity window in days.
    window: u32,
    /// Emit markdown instead of a table.
    markdown: bool,
    /// Rank by personal relevance.
    personal: bool,
    /// Send via Telegram.
    telegram: bool,
    /// Prepend an AI briefing.
    ai: bool,
    /// Append integrity sections.
    integrity: bool,
    /// Trending topics count (0 = off).
    topics: u32,
    /// Kind filter (empty = all).
    kinds: Vec<String>,
    /// Persistence threshold in distinct days (0 = off).
    persistent: u32,
    /// Shape-analysis threshold in distinct days (0 = off).
    shapes: u32,
    /// Forecast threshold in history points (0 = off): project the
    /// next-day delta for subjects with >= N daily points.
    forecast: u32,
    /// Write the rendered report to this file.
    out: Option<std::path::PathBuf>,
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    let settings = match config::load(cli.config.as_deref()) {
        Ok(settings) => settings,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::FAILURE;
        }
    };

    match run(cli.command, settings).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

async fn run(command: Command, settings: config::Settings) -> Result<()> {
    match command {
        Command::Collect {
            source,
            pages,
            window_days,
            no_profile,
        } => cmd_collect(settings, source, pages, window_days, no_profile).await,
        Command::Report {
            top,
            window,
            markdown,
            personal,
            kinds,
            persistent,
            shapes,
            forecast,
            integrity,
            topics,
            ai,
            telegram,
            out,
        } => {
            cmd_report(
                settings,
                ReportOpts {
                    top,
                    window,
                    markdown,
                    personal,
                    kinds,
                    persistent,
                    shapes,
                    forecast,
                    integrity,
                    topics,
                    ai,
                    telegram,
                    out,
                },
            )
            .await
        }
        Command::Forecast { min_points, db } => cmd_forecast(settings, min_points, db),
        Command::Profile { cmd } => cmd_profile(settings, cmd).await,
    }
}

fn token_of(settings: &config::Settings) -> Option<String> {
    settings
        .github_token
        .as_ref()
        .map(Sensitive::expose)
        .cloned()
}

fn open_store(settings: &config::Settings) -> Result<Store> {
    Store::open(&settings.db_path).context("open snapshot store")
}

fn warn_if_unauthenticated(settings: &config::Settings) {
    if settings.github_token.is_none() {
        eprintln!(
            "warning: no TRENDKIT_GITHUB_TOKEN / GH_TOKEN — unauthenticated search is 10 req/min and heavily shared"
        );
    }
}

async fn cmd_collect(
    settings: config::Settings,
    sel: SourceSel,
    pages: Option<u32>,
    window_days: Option<u32>,
    no_profile: bool,
) -> Result<()> {
    warn_if_unauthenticated(&settings);
    let store = Arc::new(open_store(&settings)?);
    let ctx = CollectCtx {
        clock: Arc::new(SystemClock),
        store: store.clone(),
        pages: pages.unwrap_or(settings.github_pages),
        window_days: window_days.unwrap_or(settings.window_days),
    };

    let selected: &[&str] = match sel {
        SourceSel::All => &[
            "github",
            "hn",
            "cratesio",
            "trending",
            "huggingface",
            "edgar",
        ],
        SourceSel::Github => &["github"],
        SourceSel::Hn => &["hn"],
        SourceSel::Cratesio => &["cratesio"],
        SourceSel::Trending => &["trending"],
        SourceSel::Huggingface => &["huggingface"],
        SourceSel::Edgar => &["edgar"],
    };

    for name in selected {
        let started = std::time::Instant::now();
        let report = match *name {
            "github" => {
                warn_if_unauthenticated(&settings);
                GithubSource::new(token_of(&settings)).collect(&ctx).await?
            }
            "hn" => HnSource::new().collect(&ctx).await?,
            "cratesio" => CratesSource::new().collect(&ctx).await?,
            "trending" => TrendingSource::new().collect(&ctx).await?,
            "huggingface" => HfSource::new().collect(&ctx).await?,
            "edgar" => EdgarSource::new().collect(&ctx).await?,
            other => anyhow::bail!("unknown source: {other}"),
        };
        println!(
            "{}: {} observations over {} page(s) in {:.1}s -> {}",
            report.source,
            report.items,
            report.pages,
            started.elapsed().as_secs_f64(),
            settings.db_path.display()
        );
    }

    if !no_profile && matches!(sel, SourceSel::All | SourceSel::Github) {
        let source = GithubSource::new(token_of(&settings));
        let profile = update_profile(&source, &settings.github_owner, &ctx.store)
            .await
            .with_context(|| format!("profile refresh for @{}", settings.github_owner))?;
        let top = profile
            .top_languages(5)
            .iter()
            .map(|(lang, n)| format!("{lang}({n})"))
            .collect::<Vec<_>>()
            .join(", ");
        println!(
            "profile: {} non-fork repos for @{} [{}]",
            profile.repo_count, settings.github_owner, top
        );
    }
    Ok(())
}

async fn cmd_report(settings: config::Settings, opts: ReportOpts) -> Result<()> {
    let store = open_store(&settings)?;
    let mut rows = store
        .velocity_rows(
            opts.window,
            if opts.kinds.is_empty() {
                opts.top
            } else {
                2_000
            },
        )
        .context("velocity query failed")?;
    if !opts.kinds.is_empty() {
        // Filter the full pool, THEN truncate: excluding a dominant kind
        // (crates.io infra) must not empty an already-limited top-N.
        let wanted: Vec<String> = opts.kinds.iter().map(|k| k.to_ascii_uppercase()).collect();
        rows.retain(|r| wanted.contains(&r.kind.tag().to_string()));
        rows.truncate(opts.top as usize);
    }
    let (subjects, observations) = store.counts().context("store counts failed")?;
    let last_run = store.last_run().context("last-run query failed")?;

    let profile_rows = if opts.personal {
        Some(store.load_profile_rows().context("profile query failed")?)
    } else {
        None
    };
    let profile = profile_rows.as_ref().map(|rows| Profile::from_repos(rows));
    if opts.personal && profile.as_ref().is_some_and(Profile::is_empty) {
        eprintln!(
            "warning: profile is empty — run `trendkit profile update` first; falling back to velocity order"
        );
    }

    let profile_line = profile.as_ref().map(|p| {
        let top = p
            .top_topics(8)
            .iter()
            .map(|(t, n)| format!("{t}({n})"))
            .collect::<Vec<_>>()
            .join(", ");
        format!("profile: {} repos, topics: {}", p.repo_count, top)
    });
    let header = report::header_lines(
        opts.window,
        &settings.db_path.display().to_string(),
        subjects,
        observations,
        last_run.as_ref().map(|(ts, s, i)| (*ts, s.as_str(), *i)),
        profile_line,
    );

    let built = report::build_rows(&rows, profile.as_ref());
    let text = if opts.markdown {
        report::render_markdown(&built, &header)
    } else {
        report::render_plain(&built, &header)
    };
    print!("{text}");

    let mut telegram_text: Option<String> = None;
    if opts.persistent > 0 {
        match store.persistent_subjects(opts.window, i64::from(opts.persistent), 2_000) {
            Ok(rows) if !rows.is_empty() => {
                let mut filtered: Vec<&_> = rows.iter().collect();
                if !opts.kinds.is_empty() {
                    let wanted: Vec<String> =
                        opts.kinds.iter().map(|k| k.to_ascii_uppercase()).collect();
                    filtered.retain(|r| wanted.contains(&r.kind.tag().to_string()));
                }
                let mut lines = vec![format!(
                    "## Persistent (>= {} distinct days)",
                    opts.persistent
                )];
                for r in filtered.iter().take(opts.top as usize) {
                    lines.push(format!(
                        "- [{}] {} — {} day(s), {} now, {:.0}/day",
                        r.kind.tag(),
                        r.key,
                        r.days,
                        r.score_now,
                        r.velocity_per_day
                    ));
                }
                let section = lines.join("\n");
                println!("\n{section}");
                telegram_text = Some(match telegram_text.take() {
                    Some(t) => format!("{t}\n\n{section}"),
                    None => format!("{text}\n\n{section}"),
                });
            }
            Ok(_) => println!("\npersistent: nothing has held N days yet"),
            Err(err) => eprintln!("warning: persistence analysis failed: {err}"),
        }
    }

    if opts.forecast > 0 {
        match forecast_section(&store, opts.forecast) {
            Ok(lines) if !lines.is_empty() => {
                let section = lines.join("\n");
                println!("\n{section}");
                telegram_text = Some(match telegram_text.take() {
                    Some(t) => format!("{t}\n\n{section}"),
                    None => format!("{text}\n\n{section}"),
                });
            }
            Ok(_) => println!("\nforecast: no subjects with enough history"),
            Err(err) => eprintln!("warning: forecast failed: {err}"),
        }
    }
    if opts.integrity {
        match integrity_sections(&store, opts.window) {
            Ok(sections) if !sections.is_empty() => {
                let block = sections.join("\n");
                println!("\n{block}");
                telegram_text = Some(match telegram_text.take() {
                    Some(t) => format!("{t}\n\n{block}"),
                    None => format!("{text}\n\n{block}"),
                });
            }
            Ok(_) => println!("\nintegrity: no flags in this window"),
            Err(err) => eprintln!("warning: integrity analysis failed: {err}"),
        }
    }

    if opts.shapes > 0 {
        match shapes_section(&store, opts.window, opts.shapes) {
            Ok(lines) if !lines.is_empty() => {
                let section = lines.join("\n");
                println!("\n{section}");
                telegram_text = Some(match telegram_text.take() {
                    Some(t) => format!("{t}\n\n{section}"),
                    None => format!("{text}\n\n{section}"),
                });
            }
            Ok(_) => println!("\nshapes: no multi-day subjects yet"),
            Err(err) => eprintln!("warning: shape analysis failed: {err}"),
        }
    }

    if opts.topics > 0 {
        match topics_section(&store, opts.window, opts.topics) {
            Ok(lines) if !lines.is_empty() => {
                let section = lines.join("\n");
                println!("\n{section}");
                telegram_text = Some(match telegram_text.take() {
                    Some(t) => format!("{t}\n\n{section}"),
                    None => format!("{text}\n\n{section}"),
                });
            }
            Ok(_) => println!("\ntopics: no tagged subjects in this window"),
            Err(err) => eprintln!("warning: topic aggregation failed: {err}"),
        }
    }

    if opts.ai {
        match ai_digest(&settings, &text).await {
            Ok(Some(briefing)) => {
                println!("\n## AI briefing\n\n{briefing}");
                let base = telegram_text.as_deref().unwrap_or(&text);
                telegram_text = Some(format!("AI briefing:\n{briefing}\n\n{base}"));
            }
            Ok(None) => eprintln!("note: no ai_api_key configured — AI briefing skipped"),
            Err(err) => eprintln!("warning: AI briefing failed: {err}"),
        }
    }

    if let Some(path) = &opts.out {
        let final_text = telegram_text.as_deref().unwrap_or(&text).to_owned();
        // A directory target gets an auto-dated filename so weekly cron
        // lines never change.
        let target: std::path::PathBuf = if path.is_dir() {
            path.join(format!("trendkit-{}.md", iso_today()))
        } else {
            path.clone()
        };
        std::fs::write(&target, final_text)
            .with_context(|| format!("write report to {}", target.display()))?;
        println!("wrote report to {}", target.display());
    }

    if opts.telegram {
        send_telegram_digest(&settings, &store, telegram_text.as_deref().unwrap_or(&text)).await?;
    }
    Ok(())
}

/// Project the next-day delta from a day-ordered score series.
/// Deltas must follow time order: `Store::daily_scores` already returns
/// points ordered by day. Sorting the scores by value instead would
/// scramble any non-monotonic series (an HN story that dips and
/// recovers) and corrupt the projection.
/// Spread multiplier applied per subject kind, calibrated on week-one
/// data (7 days, ~950 subjects) by leave-one-day-out cross-validation:
/// the multiplier was fit on the first five daily deltas and scored on
/// the held-out last day. Values are the fit m68 rounded to one decimal,
/// floored at 1.0 so bands never claim false precision. Receipts
/// (held-out coverage at the fit multiplier): crate 51 percent, github
/// 74 percent, hn 77 percent, hf 48 percent. Recalibrate weekly.
fn spread_multiplier(kind: trendkit_core::SourceKind) -> f64 {
    use trendkit_core::SourceKind::*;
    match kind {
        Crate => 1.1,
        GithubRepo => 1.9,
        HfModel => 1.5,
        HnStory => 1.0,
        // No calibration data yet: filings are discrete and sparse, so
        // start wide and tighten from held-out receipts as they land.
        Edgar => 2.0,
    }
}

/// Project the next-day delta from a day-ordered score series.
///
/// Estimator per kind, from week-one backtests: cumulative trending
/// counters (crate downloads, github stars, hn points) are projected
/// as a random walk (tomorrow = today); the smoothed mean lags the
/// trend line (crate MAE 928k vs 251k, hn 16.5 vs 0.5). HF download
/// dynamics are noisy-multiplicative rather than trending, where the
/// mean wins on both MAE (36.9k vs 43.7k) and held-out band coverage
/// (48 vs 29 percent). Deltas follow time order: `Store::daily_scores`
/// already returns points ordered by day. Sorting the scores by value
/// instead would scramble any non-monotonic series and corrupt the
/// projection. The spread band is widened by the per-kind calibrated
/// [`spread_multiplier`].
fn project_series(
    kind: trendkit_core::SourceKind,
    scores: &[i64],
) -> signalkit_core::score::Projection {
    let deltas: Vec<f64> = scores
        .windows(2)
        .map(|pair| (pair[1] - pair[0]) as f64)
        .collect();
    let mut projection = if kind == trendkit_core::SourceKind::HfModel {
        signalkit_core::score::project_next(&deltas)
    } else {
        signalkit_core::score::project_next_walk(&deltas)
    };
    let m = spread_multiplier(kind);
    projection.low = projection.delta - m * (projection.delta - projection.low);
    projection.high = projection.delta + m * (projection.high - projection.delta);
    projection
}

/// Classify multi-day subjects into accelerating/steady/fading lists.
/// Project tomorrow's delta per subject from its daily score series.
/// Score every persisted forecast whose target day has arrived, then
/// project and persist the next batch. Returns the forecast lines plus
/// a verification block when scored rows exist.
///
/// Day semantics: `made_on` is the latest observed day, `for_day` the
/// one after it. On the next morning's run the target day's snapshot
/// has landed, so the observed delta is `score(for_day) - score(prev)`
/// and the row becomes falsifiable evidence instead of a vaporized
/// Telegram line.
fn forecast_section(store: &Store, min_points: u32) -> trendkit_core::Result<Vec<String>> {
    use std::collections::BTreeMap;

    let series = store.daily_scores(30)?;
    // (kind, key) -> day-ordered (day, score) pairs.
    let mut per_subject: BTreeMap<(trendkit_core::SourceKind, String), Vec<(String, i64)>> =
        BTreeMap::new();
    for point in series {
        per_subject
            .entry((point.kind, point.key))
            .or_default()
            .push((point.day.clone(), point.score));
    }

    // 1. Score due forecasts against observed deltas.
    let made_on = per_subject
        .values()
        .flat_map(|pairs| pairs.iter().map(|(d, _)| d.clone()))
        .max()
        .ok_or_else(|| trendkit_core::TrendError::Config("no observations".to_owned()))?;
    let for_day = next_iso_day(&made_on)
        .ok_or_else(|| trendkit_core::TrendError::Config(format!("bad max day: {made_on}")))?;
    let mut scored = 0usize;
    let mut hits = 0usize;
    for due in store.due_forecasts(&made_on)? {
        let Some(pairs) = per_subject.get(&(due.kind, due.key.clone())) else {
            continue; // subject left the window: row stays unscored
        };
        let Some(pos) = pairs.iter().position(|(d, _)| *d == due.for_day) else {
            continue; // no snapshot on the target day yet
        };
        if pos == 0 {
            continue; // no prior day to difference against
        }
        let actual = (pairs[pos].1 - pairs[pos - 1].1) as f64;
        let band_hit = actual >= due.low && actual <= due.high;
        store.score_forecast(due.id, actual, band_hit)?;
        scored += 1;
        hits += usize::from(band_hit);
    }

    // 2. Verification block from all scored history.
    let history = store.scored_forecasts()?;
    let mut lines: Vec<String> = Vec::new();
    if !history.is_empty() {
        let n = history.len();
        let mae: f64 = history
            .iter()
            .map(|(_, p, a, _)| (p - a).abs())
            .sum::<f64>()
            / n as f64;
        let band = history.iter().filter(|(.., h)| *h).count();
        lines.push(format!(
            "## Forecast verification: {n} scored, MAE {mae:.0}, band hit {band}/{n} ({:.0}%)",
            100.0 * band as f64 / n as f64
        ));
        let mut by_kind: BTreeMap<&str, (usize, f64, usize)> = BTreeMap::new();
        for (kind, p, a, h) in &history {
            let e = by_kind.entry(kind.as_str()).or_default();
            e.0 += 1;
            e.1 += (p - a).abs();
            e.2 += usize::from(*h);
        }
        for (kind, (k_n, k_err, k_hits)) in by_kind {
            lines.push(format!(
                "- {kind}: {k_n} scored, MAE {:.0}, band hit {k_hits}/{k_n}",
                k_err / k_n as f64
            ));
        }
        lines.push(String::new());
    }

    // 3. Project and persist the next batch.
    let mut rows: Vec<(f64, String, String, f64, f64)> = Vec::new();
    let mut inputs: Vec<trendkit_core::model::ForecastInput<'_>> = Vec::new();
    for ((kind, key), pairs) in &per_subject {
        if (pairs.len() as u32) < min_points {
            continue;
        }
        let scores: Vec<i64> = pairs.iter().map(|(_, s)| *s).collect();
        let projection = project_series(*kind, &scores);
        let estimator = if *kind == trendkit_core::SourceKind::HfModel {
            "mean"
        } else {
            "walk"
        };
        inputs.push(trendkit_core::model::ForecastInput {
            kind: *kind,
            key,
            projected: projection.delta,
            low: projection.low,
            high: projection.high,
            estimator,
        });
        rows.push((
            projection.delta,
            format!("{kind:?}"),
            key.clone(),
            projection.low,
            projection.high,
        ));
    }
    if rows.is_empty() {
        if !lines.is_empty() {
            return Ok(lines);
        }
        return Ok(Vec::new());
    }
    store.record_forecasts(&made_on, &for_day, &inputs)?;
    rows.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    lines.push(format!(
        "## Forecast: projected next-day delta for {for_day}, top {} of {} (walk or mean per kind, calibrated band)",
        rows.len().min(50),
        rows.len()
    ));
    for (delta, kind, key, low, high) in rows.iter().take(50) {
        lines.push(format!(
            "- [{kind}] {key} — projected {delta:+.0}/day (band {low:.0} to {high:.0})"
        ));
    }
    lines.push(format!(
        "- persisted {} forecasts made on {made_on}; scored {scored} due today ({hits} band hits)",
        inputs.len()
    ));
    Ok(lines)
}

/// The calendar day after an ISO `YYYY-MM-DD` date.
fn next_iso_day(day: &str) -> Option<String> {
    let mut parts = day.split('-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m: i64 = parts.next()?.parse().ok()?;
    let d: i64 = parts.next()?.parse().ok()?;
    // Hinnant's days_from_civil, then civil_from_days + 1.
    let y_adj = if m <= 2 { y - 1 } else { y };
    let era = y_adj.div_euclid(400);
    let yoe = y_adj - era * 400;
    let mp = (m + 9).rem_euclid(12);
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468 + 1;
    let z = days + 719_468;
    let era2 = z.div_euclid(146_097);
    let doe2 = z - era2 * 146_097;
    let yoe2 = (doe2 - doe2 / 1460 + doe2 / 36_524 - doe2 / 146_096) / 365;
    let y2 = yoe2 + era2 * 400;
    let doy2 = doe2 - (365 * yoe2 + yoe2 / 4 - yoe2 / 100);
    let mp2 = (5 * doy2 + 2) / 153;
    let d2 = doy2 - (153 * mp2 + 2) / 5 + 1;
    let m2 = if mp2 < 10 { mp2 + 3 } else { mp2 - 9 };
    let y3 = if m2 <= 2 { y2 + 1 } else { y2 };
    Some(format!("{y3:04}-{m2:02}-{d2:02}"))
}

fn shapes_section(store: &Store, window: u32, min_days: u32) -> trendkit_core::Result<Vec<String>> {
    use signalkit_integrity::classify_shape;
    use std::collections::BTreeMap;

    let series = store.daily_scores(window)?;
    let mut per_subject: BTreeMap<(trendkit_core::SourceKind, String), Vec<(String, i64)>> =
        BTreeMap::new();
    for point in series {
        per_subject
            .entry((point.kind, point.key))
            .or_default()
            .push((point.day, point.score));
    }

    let mut accelerating: Vec<(String, String, i64, i64)> = Vec::new();
    let mut fading: Vec<(String, String, i64, i64)> = Vec::new();
    for ((kind, key), mut points) in per_subject {
        if points.len() < 2 {
            continue;
        }
        points.sort();
        let velocities: Vec<f64> = points
            .windows(2)
            .map(|pair| (pair[1].1 - pair[0].1) as f64)
            .collect();
        let latest_delta = points
            .windows(2)
            .last()
            .map(|pair| pair[1].1 - pair[0].1)
            .unwrap_or(0);
        match classify_shape(&velocities) {
            signalkit_integrity::TrendShape::Accelerating => accelerating.push((
                format!("{kind:?}"),
                key,
                latest_delta,
                points.last().map(|p| p.1).unwrap_or(0),
            )),
            signalkit_integrity::TrendShape::Fading => fading.push((
                format!("{kind:?}"),
                key,
                latest_delta,
                points.last().map(|p| p.1).unwrap_or(0),
            )),
            _ => {}
        }
    }
    if accelerating.is_empty() && fading.is_empty() {
        return Ok(Vec::new());
    }
    accelerating.sort_by(|a, b| b.3.cmp(&a.3).then_with(|| b.2.cmp(&a.2)));
    fading.sort_by(|a, b| b.3.cmp(&a.3).then_with(|| b.2.cmp(&a.2)));

    let mut lines = vec!["## Shapes (multi-day subjects)".to_owned()];
    for (kind_tag, key, latest_delta, score) in accelerating.iter().take(min_days as usize) {
        lines.push(format!(
            "- ACCELERATING [{kind_tag}] {key} — {score} now, latest day +{latest_delta}"
        ));
    }
    // "Fading" here means the daily delta is shrinking — the subject may
    // still be growing strongly in absolute terms, so say "cooling".
    for (kind_tag, key, latest_delta, score) in fading.iter().take(min_days as usize) {
        lines.push(format!(
            "- COOLING [{kind_tag}] {key} — {score} now, latest day +{latest_delta}"
        ));
    }
    Ok(lines)
}

fn integrity_sections(store: &Store, window: u32) -> trendkit_core::Result<Vec<String>> {
    let mut sections = Vec::new();

    let trust: std::collections::BTreeMap<String, (f64, &'static str)> =
        signalkit_integrity::trust_rows(store, window, 200)?
            .into_iter()
            .map(|r| {
                let band = match r.summary.band {
                    signalkit_integrity::TrustBand::Trusted => "trusted",
                    signalkit_integrity::TrustBand::Watch => "watch",
                    signalkit_integrity::TrustBand::Suspect => "SUSPECT",
                };
                (r.key, (r.summary.score, band))
            })
            .collect();

    let flags = signalkit_integrity::fork_flags(store, window, 200)?;
    if !flags.is_empty() {
        let mut lines = vec!["## Integrity: fork-ratio flags".to_owned()];
        for f in &flags {
            let trust_line = trust.get(&f.key).map_or(String::new(), |(score, band)| {
                format!(" — trust {:.0} [{band}]", score)
            });
            lines.push(format!(
                "- {} — {}★/{} forks ({:.1}%) [{}]{}",
                f.key,
                f.stars,
                f.forks,
                f.ratio_pct,
                match f.verdict {
                    signalkit_integrity::ForkVerdict::Elevated => "elevated",
                    signalkit_integrity::ForkVerdict::Extreme => "extreme",
                    signalkit_integrity::ForkVerdict::Normal => "normal",
                },
                trust_line
            ));
        }
        sections.push(lines.join("\n"));
    }

    let clones = signalkit_integrity::name_clones(store, window, 3, 40.0)?;
    if !clones.is_empty() {
        let mut lines = vec!["## Integrity: same-name clone farms".to_owned()];
        for c in clones.iter().take(5) {
            lines.push(format!(
                "- `{}` — {} orgs, {}★/{} forks ({:.1}%)",
                c.name,
                c.orgs.len(),
                c.total_stars,
                c.total_forks,
                c.avg_ratio_pct
            ));
        }
        sections.push(lines.join("\n"));
    }

    let bursts = signalkit_integrity::org_bursts(store, window, 3, 25.0, 5_000.0)?;
    if !bursts.is_empty() {
        let mut lines = vec!["## Integrity: org bursts".to_owned()];
        for b in bursts.iter().take(5) {
            lines.push(format!(
                "- @{} — {} repos, combined {:.0}/day: {}",
                b.owner,
                b.subjects,
                b.combined_velocity,
                b.keys
                    .iter()
                    .take(4)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        sections.push(lines.join("\n"));
    }

    let outliers = signalkit_integrity::velocity_outliers(store, window, 500)?;
    if !outliers.is_empty() {
        let mut lines = vec!["## Integrity: velocity outliers".to_owned()];
        for o in outliers.iter().take(5) {
            lines.push(format!(
                "- [{}] {} — {:.1}/day (robust z {:.1})",
                o.kind.tag(),
                o.key,
                o.velocity_per_day,
                o.robust_z
            ));
        }
        sections.push(lines.join("\n"));
    }
    Ok(sections)
}

/// Aggregate the highest-delta topics across the window's subjects.
fn topics_section(store: &Store, window: u32, top_n: u32) -> trendkit_core::Result<Vec<String>> {
    use std::collections::BTreeMap;

    let rows = store.velocity_rows(window, 500)?;
    let mut agg: BTreeMap<String, (usize, i64)> = BTreeMap::new();
    for row in &rows {
        let delta = row.score_now - row.score_prev;
        for topic in &row.topics {
            let entry = agg.entry(topic.clone()).or_default();
            entry.0 += 1;
            entry.1 += delta;
        }
    }
    let mut items: Vec<(String, usize, i64)> =
        agg.into_iter().map(|(t, (c, d))| (t, c, d)).collect();
    items.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
    if items.is_empty() {
        return Ok(Vec::new());
    }
    let mut lines = vec![format!("## Trending topics (top {top_n})")];
    for (topic, repos, delta) in items.iter().take(top_n as usize) {
        lines.push(format!(
            "- {topic} — {repos} repo(s), +{delta} stars across tracked subjects"
        ));
    }
    Ok(lines)
}

/// Fetch the AI briefing; `Ok(None)` when no key is configured.
async fn ai_digest(settings: &config::Settings, report_text: &str) -> Result<Option<String>> {
    let Some(key) = settings.ai_api_key.as_ref() else {
        return Ok(None);
    };
    let config = trendkit_core::ai::AiDigestConfig {
        base_url: settings.ai_base_url.clone(),
        api_key: key.expose().to_owned(),
        model: settings.ai_model.clone(),
    };
    Ok(Some(
        trendkit_core::ai::summarize(&config, report_text).await?,
    ))
}

/// Today's UTC date as `YYYY-MM-DD` (for auto-dated report filenames).
fn iso_today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs / 86_400;
    // Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

fn telegram_token(settings: &config::Settings) -> Result<String> {
    if let Ok(token) = std::env::var("TRENDKIT_TELEGRAM_TOKEN") {
        if !token.trim().is_empty() {
            return Ok(token);
        }
    }
    let path = &settings.telegram_token_file;
    let token = std::fs::read_to_string(path)
        .with_context(|| format!("read telegram token file {}", path.display()))?
        .trim()
        .to_owned();
    if token.is_empty() {
        anyhow::bail!("telegram token file {} is empty", path.display());
    }
    Ok(token)
}

fn chat_id_of(settings: &config::Settings) -> Result<String> {
    if let Ok(id) = std::env::var("TRENDKIT_TELEGRAM_CHAT_ID") {
        if !id.trim().is_empty() {
            return Ok(id);
        }
    }
    settings
        .telegram_chat_id
        .clone()
        .filter(|id| !id.trim().is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no telegram chat id — set telegram_chat_id in trendkit.toml or TRENDKIT_TELEGRAM_CHAT_ID"
            )
        })
}

async fn send_telegram_digest(
    settings: &config::Settings,
    store: &Store,
    report_text: &str,
) -> Result<()> {
    let outcome = deliver_digest(settings, report_text).await;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    match &outcome {
        Ok(chat_id) => {
            if let Err(err) = store.record_digest(ts, true, "") {
                eprintln!("warning: could not record digest success: {err}");
            }
            println!("telegram: digest sent to chat {chat_id}");
        }
        Err(err) => {
            let detail = format!("{err:#}");
            // Keep the detail short enough for a row and an alert line.
            let detail = detail.chars().take(300).collect::<String>();
            if let Err(log_err) = store.record_digest(ts, false, &detail) {
                eprintln!("warning: could not record digest failure: {log_err}");
            }
            eprintln!("telegram: digest failed: {detail}");
        }
    }
    outcome.map(|_| ())
}

/// Resolve credentials, render, and deliver the digest.
async fn deliver_digest(settings: &config::Settings, report_text: &str) -> Result<String> {
    let token = telegram_token(settings)?;
    let chat_id = chat_id_of(settings)?;
    let message = trendkit_core::telegram::truncate_for_telegram(
        report_text,
        trendkit_core::telegram::TELEGRAM_MAX_CHARS,
    );
    trendkit_core::telegram::send_markdown(&token, &chat_id, &message)
        .await
        .context("telegram send failed")?;
    Ok(chat_id)
}

fn cmd_forecast(settings: config::Settings, min_points: u32, db: PathBuf) -> Result<()> {
    let _ = settings;
    let store = Store::open(&db).context("open store")?;
    let series = store.daily_scores(30)?;

    // Group per subject
    let mut per_subject: std::collections::BTreeMap<
        (trendkit_core::SourceKind, String),
        Vec<(String, i64)>,
    > = std::collections::BTreeMap::new();
    for point in series {
        per_subject
            .entry((point.kind, point.key))
            .or_default()
            .push((point.day, point.score));
    }

    let mut projections: Vec<(f64, f64, f64, String, String, i64)> = Vec::new();
    for ((kind, key), mut points) in per_subject {
        if points.len() < min_points as usize {
            continue;
        }
        points.sort();
        let scores: Vec<i64> = points.iter().map(|(_, s)| *s).collect();
        let projection = project_series(kind, &scores);
        let latest = *scores.last().unwrap_or(&0);
        projections.push((
            projection.delta,
            projection.confidence,
            projection.low,
            format!("{kind:?}"),
            key,
            latest,
        ));
    }

    if projections.is_empty() {
        println!("forecast: no subjects with >= {min_points} daily points");
        return Ok(());
    }

    projections.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    println!(
        "Forecast: projected next-day delta, top {} of {} subjects (walk or mean per kind, per-kind calibrated band)\n",
        projections.len().min(20),
        projections.len()
    );
    println!(
        "{:<6} {:<36} {:>10} {:>10} {:>8}",
        "KIND", "SUBJECT", "LATEST", "PROJECTED", "CONF"
    );
    println!("{}", "-".repeat(80));
    for (delta, conf, _low, kind, key, latest) in projections.iter().take(20) {
        println!("{kind:>8} {key:<36} {latest:>10} {delta:>+10.0} {conf:>8.2}");
    }
    println!(
        "\nNote: naive extrapolation with < 1 week of history.\nTreat as directional, not predictive.\nConfidence scales with history length (max 1.00 at 7 days)."
    );
    Ok(())
}

async fn cmd_profile(settings: config::Settings, cmd: ProfileCommand) -> Result<()> {
    match cmd {
        ProfileCommand::Update { owner } => {
            warn_if_unauthenticated(&settings);
            let owner = owner.unwrap_or_else(|| settings.github_owner.clone());
            let store = open_store(&settings)?;
            let source = GithubSource::new(token_of(&settings));
            let profile = update_profile(&source, &owner, &store)
                .await
                .with_context(|| format!("profile refresh for @{owner}"))?;
            println!("profile @{}: {} repos", owner, profile.repo_count);
            print_profile_summary(&profile);
        }
        ProfileCommand::Show => {
            let store = open_store(&settings)?;
            let rows = store.load_profile_rows().context("profile query failed")?;
            if rows.is_empty() {
                println!("no stored profile — run `trendkit profile update`");
                return Ok(());
            }
            let profile = Profile::from_repos(&rows);
            println!(
                "profile: {} repos (from {} stored rows)",
                profile.repo_count,
                rows.len()
            );
            print_profile_summary(&profile);
        }
    }
    Ok(())
}

fn print_profile_summary(profile: &Profile) {
    for (label, items) in [
        ("languages", profile.top_languages(10)),
        ("topics", profile.top_topics(10)),
    ] {
        let line = items
            .iter()
            .map(|(k, n)| format!("{k}({n})"))
            .collect::<Vec<_>>()
            .join(", ");
        println!("{label}: {line}");
    }
}

#[cfg(test)]
mod forecast_tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::{forecast_section, project_series, spread_multiplier};
    use signalkit_core::score::project_next;
    use trendkit_core::model::{Observation, SourceKind, SubjectRecord};
    use trendkit_core::store::Store;

    /// Regression: deltas must follow time order, not value order.
    /// Under the walk estimator the point projection survives a value
    /// sort (both read the last delta as +10) but the band does not:
    /// the scrambled deltas [-20, +10] carry real variance while the
    /// value-sorted [10, 10] collapse the spread to zero.
    #[test]
    fn deltas_follow_time_order_not_value_order() {
        let p = project_series(SourceKind::HnStory, &[50, 30, 40]);
        assert!((p.delta - 10.0).abs() < 1e-9, "delta {}", p.delta);
        let scrambled = signalkit_core::score::project_next_walk(&[10.0, 10.0]);
        assert!(
            p.high - p.low > scrambled.high - scrambled.low,
            "time-ordered band {} must exceed the value-sorted band {}",
            p.high - p.low,
            scrambled.high - scrambled.low
        );
    }

    /// Monotonic series are unaffected by the fix: value order and time
    /// order coincide, and the crate multiplier widens the raw walk
    /// band by exactly 1.1x.
    #[test]
    fn monotonic_series_unchanged() {
        let p = project_series(SourceKind::Crate, &[100, 150, 220]);
        let raw = signalkit_core::score::project_next_walk(&[50.0, 70.0]);
        assert!((p.delta - 70.0).abs() < 1e-9);
        assert!(((p.high - p.low) - 1.1 * (raw.high - raw.low)).abs() < 1e-9);
    }

    /// HF models keep the smoothed-mean estimator (backtest: mean wins
    /// on MAE and coverage there), unbwidened beyond the 1.5x
    /// multiplier, while crates use the walk.
    #[test]
    fn estimator_selection_by_kind() {
        let hf = project_series(SourceKind::HfModel, &[100, 150, 220]);
        let expected_mean = project_next(&[50.0, 70.0]);
        assert!((hf.delta - expected_mean.delta).abs() < 1e-9);
        assert!(
            (hf.delta - 63.0).abs() < 1.0,
            "mean lags walk: {}",
            hf.delta
        );

        let crate_p = project_series(SourceKind::Crate, &[100, 150, 220]);
        assert!(
            (crate_p.delta - 70.0).abs() < 1e-9,
            "walk: {}",
            crate_p.delta
        );
    }

    /// The calibration table matches the week-one LOO receipts.
    #[test]
    fn multiplier_table_matches_calibration() {
        assert!((spread_multiplier(SourceKind::Crate) - 1.1).abs() < 1e-9);
        assert!((spread_multiplier(SourceKind::GithubRepo) - 1.9).abs() < 1e-9);
        assert!((spread_multiplier(SourceKind::HfModel) - 1.5).abs() < 1e-9);
        assert!((spread_multiplier(SourceKind::HnStory) - 1.0).abs() < 1e-9);
    }

    /// End to end: daily_scores returns day-ordered points, so the
    /// section projects from the honest timeline, persists the batch,
    /// and on the next day's run scores it against the observed delta.
    #[test]
    fn forecast_section_persists_then_scores() {
        let store = Store::open_in_memory().unwrap();
        let id = store
            .upsert_subject(&SubjectRecord {
                kind: SourceKind::HnStory,
                key: "story1".to_string(),
                title: None,
                lang: None,
                url: None,
                topics: vec![],
                created_at: None,
            })
            .unwrap();
        let base: i64 = 1_770_000_000;
        for (day, score) in [(0, 50), (1, 30), (2, 40)] {
            store
                .insert_observation(
                    id,
                    &Observation {
                        ts: base + i64::from(day) * 86_400,
                        score,
                        extra1: 0,
                        extra2: 0,
                    },
                )
                .unwrap();
        }

        // Day 3: projects for 2026-02-05 and persists the batch.
        let lines = forecast_section(&store, 3).unwrap();
        assert_eq!(lines.len(), 3, "header, row, persisted: {lines:?}");
        assert!(lines[0].contains("for 2026-02-05"), "{}", lines[0]);
        let p = project_series(SourceKind::HnStory, &[50, 30, 40]);
        let fragment = format!("projected {:+.0}/day", p.delta);
        assert!(lines[1].contains(&fragment), "{}", lines[1]);

        // Day 4 run: the target day's snapshot (score 60) has landed,
        // so the stored forecast scores as actual +20, a band hit
        // (band was -4 to 24), and verification renders.
        store
            .insert_observation(
                id,
                &Observation {
                    ts: base + 3 * 86_400,
                    score: 60,
                    extra1: 0,
                    extra2: 0,
                },
            )
            .unwrap();
        let lines = forecast_section(&store, 3).unwrap();
        let verify = lines
            .iter()
            .find(|l| l.contains("Forecast verification"))
            .expect("verification block");
        assert!(verify.contains("1 scored"), "{}", verify);
        assert!(verify.contains("band hit 1/1"), "{}", verify);
    }

    /// The day-after helper rolls months and years correctly.
    #[test]
    fn next_iso_day_rolls() {
        assert_eq!(
            super::next_iso_day("2026-10-25").as_deref(),
            Some("2026-10-26")
        );
        assert_eq!(
            super::next_iso_day("2026-10-31").as_deref(),
            Some("2026-11-01")
        );
        assert_eq!(
            super::next_iso_day("2026-12-31").as_deref(),
            Some("2027-01-01")
        );
        assert_eq!(
            super::next_iso_day("2028-02-28").as_deref(),
            Some("2028-02-29")
        );
        assert_eq!(super::next_iso_day("not-a-date"), None);
    }
}

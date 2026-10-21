//! trendkit — dev-trend crawler CLI.
//!
//! Subcommands:
//! - `collect` — snapshot observations into the SQLite store
//! - `report`  — velocity ranking (optionally personal-scored)
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
        /// Also send the markdown report via Telegram.
        #[arg(long, default_value_t = false)]
        telegram: bool,
        /// Prepend an AI briefing (needs ai_api_key; skips if absent).
        #[arg(long, default_value_t = false)]
        ai: bool,
        /// Also write the rendered report to this file (markdown mode
        /// recommended) — for weekly public pages.
        #[arg(long)]
        out: Option<std::path::PathBuf>,
        /// Append integrity signals (fork flags, org bursts, outliers).
        #[arg(long, default_value_t = false)]
        integrity: bool,
        /// Append the top-N trending topics across the window.
        #[arg(long, default_value_t = 0)]
        topics: u32,
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
            telegram,
            ai,
            integrity,
            topics,
            kinds,
            persistent,
            out,
        } => {
            cmd_report(
                settings,
                ReportOpts {
                    top,
                    window,
                    markdown,
                    personal,
                    telegram,
                    ai,
                    integrity,
                    topics,
                    kinds,
                    persistent,
                    out,
                },
            )
            .await
        }
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
    let store = Arc::new(open_store(&settings)?);
    let ctx = CollectCtx {
        clock: Arc::new(SystemClock),
        store: store.clone(),
        pages: pages.unwrap_or(settings.github_pages),
        window_days: window_days.unwrap_or(settings.window_days),
    };

    let selected: &[&str] = match sel {
        SourceSel::All => &["github", "hn", "cratesio", "trending", "huggingface"],
        SourceSel::Github => &["github"],
        SourceSel::Hn => &["hn"],
        SourceSel::Cratesio => &["cratesio"],
        SourceSel::Trending => &["trending"],
        SourceSel::Huggingface => &["huggingface"],
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
    /// Write the rendered report to this file.
    out: Option<std::path::PathBuf>,
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
                for r in &filtered {
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
        std::fs::write(path, final_text)
            .with_context(|| format!("write report to {}", path.display()))?;
        println!("wrote report to {}", path.display());
    }

    if opts.telegram {
        send_telegram_digest(&settings, telegram_text.as_deref().unwrap_or(&text)).await?;
    }
    Ok(())
}

/// Aggregate the highest-delta topics across the window's subjects.
/// `delta` per subject = latest minus earliest score inside the window.
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

/// Build the integrity sections (fork flags, org bursts, outliers) for
/// the report tail; empty when nothing is flagged.
fn integrity_sections(store: &Store, window: u32) -> trendkit_core::Result<Vec<String>> {
    let mut sections = Vec::new();
    let trust: std::collections::BTreeMap<String, (f64, &'static str)> =
        signalkit_integrity::trust_rows(store, window, 200)?
            .into_iter()
            .map(|r| {
                // The summary band already carries the extreme-verdict cap.
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

    let mut suspects: Vec<(&String, &(f64, &'static str))> =
        trust.iter().filter(|(_, (s, _))| *s < 80.0).collect();
    suspects.sort_by(|a, b| {
        a.1 .0
            .partial_cmp(&b.1 .0)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if !suspects.is_empty() {
        let mut lines = vec!["## Integrity: trust suspects (composite < 80)".to_owned()];
        for (key, (score, band)) in suspects.iter().take(5) {
            lines.push(format!("- {key} — {:.0} [{band}]", score));
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

/// Build the AI briefing; `Ok(None)` when no key is configured.
async fn ai_digest(settings: &config::Settings, report_text: &str) -> Result<Option<String>> {
    let Some(key) = settings.ai_api_key.as_ref() else {
        return Ok(None);
    };
    let config = trendkit_core::ai::AiDigestConfig {
        base_url: settings.ai_base_url.clone(),
        api_key: key.expose().to_owned(),
        model: settings.ai_model.clone(),
    };
    trendkit_core::ai::summarize(&config, report_text)
        .await
        .map(Some)
        .map_err(Into::into)
}

/// Resolve the bot token: env override, else the fleet token file.
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

async fn send_telegram_digest(settings: &config::Settings, report_text: &str) -> Result<()> {
    let token = telegram_token(settings)?;
    let chat_id = chat_id_of(settings)?;
    let message = trendkit_core::telegram::truncate_for_telegram(
        report_text,
        trendkit_core::telegram::TELEGRAM_MAX_CHARS,
    );
    // Markdown rendering is the transport format; the plain report's
    // table characters are harmless to the legacy parser.
    trendkit_core::telegram::send_markdown(&token, &chat_id, &message)
        .await
        .context("telegram send failed")?;
    println!("telegram: digest sent to chat {chat_id}");
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

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
        /// Also send the markdown report via Telegram.
        #[arg(long, default_value_t = false)]
        telegram: bool,
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
        } => cmd_report(settings, top, window, markdown, personal, telegram).await,
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
        SourceSel::All => &["github", "hn", "cratesio", "trending"],
        SourceSel::Github => &["github"],
        SourceSel::Hn => &["hn"],
        SourceSel::Cratesio => &["cratesio"],
        SourceSel::Trending => &["trending"],
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

async fn cmd_report(
    settings: config::Settings,
    top: u32,
    window: u32,
    markdown: bool,
    personal: bool,
    telegram: bool,
) -> Result<()> {
    let store = open_store(&settings)?;
    let rows = store
        .velocity_rows(window, top)
        .context("velocity query failed")?;
    let (subjects, observations) = store.counts().context("store counts failed")?;
    let last_run = store.last_run().context("last-run query failed")?;

    let profile_rows = if personal {
        Some(store.load_profile_rows().context("profile query failed")?)
    } else {
        None
    };
    let profile = profile_rows.as_ref().map(|rows| Profile::from_repos(rows));
    if personal && profile.as_ref().is_some_and(Profile::is_empty) {
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
        window,
        &settings.db_path.display().to_string(),
        subjects,
        observations,
        last_run.as_ref().map(|(ts, s, i)| (*ts, s.as_str(), *i)),
        profile_line,
    );

    let built = report::build_rows(&rows, profile.as_ref());
    let text = if markdown {
        report::render_markdown(&built, &header)
    } else {
        report::render_plain(&built, &header)
    };
    print!("{text}");

    if telegram {
        send_telegram_digest(&settings, &text).await?;
    }
    Ok(())
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

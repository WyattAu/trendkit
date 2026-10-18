//! Layered settings via `config-kit`.
//!
//! Flat by design: config-kit v0.1.x maps env vars onto **top-level**
//! fields (`TRENDKIT_GITHUB_TOKEN` → `github_token`), so nesting the
//! struct would silently break the env layer. Precedence follows the
//! crate contract: CLI flags (applied by the caller) > env > file.
//! The GitHub token is a `Sensitive<String>` and renders redacted in any
//! `Debug` output.

use std::path::{Path, PathBuf};

use config_kit::{ConfigBuilder, ConfigLayer, Sensitive};
use serde::Deserialize;
use trendkit_core::error::{Result, TrendError};

/// trendkit settings.
#[derive(Debug, Deserialize)]
pub struct Settings {
    /// SQLite database path (default `./trendkit.db`).
    #[serde(default = "default_db_path")]
    pub db_path: PathBuf,
    /// GitHub PAT; falls back to `GH_TOKEN` when absent.
    #[serde(default)]
    pub github_token: Option<Sensitive<String>>,
    /// GitHub owner whose repos define the personal profile.
    #[serde(default = "default_owner")]
    pub github_owner: String,
    /// Search pages per collection run (100 repos each).
    #[serde(default = "default_pages")]
    pub github_pages: u32,
    /// "Recent window" for trending queries and report velocity, in days.
    #[serde(default = "default_window_days")]
    pub window_days: u32,
    /// Telegram chat id for `report --telegram`.
    #[serde(default)]
    pub telegram_chat_id: Option<String>,
    /// File containing the Telegram bot token (fleet convention).
    #[serde(default = "default_token_file")]
    pub telegram_token_file: PathBuf,
}

fn default_db_path() -> PathBuf {
    PathBuf::from("trendkit.db")
}

fn default_owner() -> String {
    "WyattAu".to_owned()
}

fn default_pages() -> u32 {
    3
}

fn default_window_days() -> u32 {
    14
}

fn default_token_file() -> PathBuf {
    PathBuf::from("/etc/alertmanager/telegram-token")
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            db_path: default_db_path(),
            github_token: None,
            github_owner: default_owner(),
            github_pages: default_pages(),
            window_days: default_window_days(),
            telegram_chat_id: None,
            telegram_token_file: default_token_file(),
        }
    }
}

/// Load settings from `config` (explicit path, must exist when given),
/// falling back to `./trendkit.toml` when present, then the
/// `TRENDKIT_`-prefixed environment, then defaults. Applies the
/// `GH_TOKEN` fallback for the token.
///
/// # Errors
/// Fails on unreadable config files, unknown strict keys, or values that
/// do not fit the [`Settings`] types.
pub fn load(config: Option<&Path>) -> Result<Settings> {
    let file_layer = match config {
        Some(path) => {
            if !path.exists() {
                return Err(TrendError::Config(format!(
                    "config file not found: {}",
                    path.display()
                )));
            }
            Some(ConfigLayer::file(path))
        }
        None => {
            let default_path = Path::new("trendkit.toml");
            if default_path.exists() {
                Some(ConfigLayer::file(default_path))
            } else {
                None
            }
        }
    };

    // `ConfigBuilder` is type-state (Empty -> Ready on first layer), so the
    // two shapes are spelled out instead of reassigned through one binding.
    let mut settings: Settings = match file_layer {
        Some(file) => ConfigBuilder::new()
            .layer(file)
            .layer(ConfigLayer::env_prefix("TRENDKIT_"))
            .load_strict(),
        None => ConfigBuilder::new()
            .layer(ConfigLayer::env_prefix("TRENDKIT_"))
            .load_strict(),
    }
    .map_err(|e| TrendError::Config(e.to_string()))?;

    if settings.github_token.is_none() {
        settings.github_token = std::env::var("GH_TOKEN").ok().map(Sensitive::new);
    }
    Ok(settings)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn defaults_are_sane() {
        let settings = Settings::default();
        assert_eq!(settings.db_path, PathBuf::from("trendkit.db"));
        assert_eq!(settings.github_owner, "WyattAu");
        assert_eq!(settings.github_pages, 3);
        assert_eq!(settings.window_days, 14);
        assert!(settings.github_token.is_none());
    }

    #[test]
    fn missing_explicit_config_is_an_error() {
        let err = load(Some(Path::new("/nonexistent/trendkit.toml"))).unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn sensitive_tokens_never_debug_print() {
        let token = Sensitive::new("hunter2".to_owned());
        assert_eq!(format!("{token:?}"), "[REDACTED]");
        assert_eq!(token.expose(), "hunter2");
    }
}

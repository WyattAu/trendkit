//! Telegram digest delivery via the Bot API.
//!
//! Matches the fleet's alerting conventions (sys-check): bot token from
//! a root-owned file (`/etc/alertmanager/telegram-token`), chat id from
//! configuration. Messages are Markdown-parsed by Telegram; the 4096
//! character hard cap is the caller's concern — see
//! `trendkit::report`-adjacent truncation helpers.

use fetch_kit::Client;
use serde::{Deserialize, Serialize};

use crate::error::{Result, TrendError};

const USER_AGENT: &str = "trendkit/0.2";
/// Telegram sendMessage hard limit.
pub const TELEGRAM_MAX_CHARS: usize = 4_096;

#[derive(Debug, Serialize)]
struct SendMessage<'a> {
    chat_id: &'a str,
    text: &'a str,
    parse_mode: &'a str,
}

#[derive(Debug, Deserialize)]
struct TelegramResponse {
    ok: bool,
    #[serde(default)]
    description: Option<String>,
}

/// Send `text` as a Markdown message to `chat_id`.
///
/// # Errors
/// Fails on transport errors and on Telegram-level rejections
/// (`ok: false`), carrying Telegram's description.
pub async fn send_markdown(token: &str, chat_id: &str, text: &str) -> Result<()> {
    let client = Client::builder().user_agent(USER_AGENT).build();
    let url = format!("https://api.telegram.org/bot{token}/sendMessage");
    let response: TelegramResponse = client
        .post_json(
            &url,
            &SendMessage {
                chat_id,
                text,
                parse_mode: "Markdown",
            },
        )
        .await?;
    if !response.ok {
        return Err(TrendError::Other(format!(
            "telegram rejected the message: {}",
            response.description.as_deref().unwrap_or("no description")
        )));
    }
    Ok(())
}

/// Truncate a report to fit Telegram's hard cap, cutting on a line
/// boundary and marking the cut.
#[must_use]
pub fn truncate_for_telegram(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    let cut: String = text.chars().take(max_chars.saturating_sub(20)).collect();
    let trimmed = match cut.rfind('\n') {
        Some(pos) if pos > max_chars / 2 => cut[..pos].to_owned(),
        _ => cut,
    };
    format!("{trimmed}\n…(truncated)")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn short_text_passes_through() {
        let text = "hello\nworld";
        assert_eq!(truncate_for_telegram(text, TELEGRAM_MAX_CHARS), text);
    }

    #[test]
    fn long_text_is_cut_on_a_line_boundary_with_marker() {
        let line = "x".repeat(100);
        let text = format!("{}\n", line.repeat(100));
        let out = truncate_for_telegram(&text, 500);
        assert!(out.chars().count() <= 500);
        assert!(out.ends_with("…(truncated)"));
        assert!(out.contains('\n'));
    }

    #[test]
    fn pathological_text_without_newlines_still_fits() {
        let text = "y".repeat(10_000);
        let out = truncate_for_telegram(&text, 300);
        assert!(out.chars().count() <= 300);
        assert!(out.ends_with("…(truncated)"));
    }
}

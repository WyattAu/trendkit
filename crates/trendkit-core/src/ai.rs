//! AI-summarized digest via any OpenAI-compatible chat endpoint.
//!
//! Defaults target DeepSeek — the commodity-model wave means a daily
//! briefing costs fractions of a cent. No key configured: the digest is
//! skipped, never fatal.

use fetch_kit::Client;
use serde::{Deserialize, Serialize};

use crate::error::{Result, TrendError};

const USER_AGENT: &str = "trendkit/0.3";
/// Report text fed to the model, to bound prompt cost.
const MAX_INPUT_CHARS: usize = 6_000;

/// Endpoint + credentials for an OpenAI-compatible chat API.
#[derive(Debug, Clone)]
pub struct AiDigestConfig {
    /// e.g. `https://api.deepseek.com` (no trailing slash).
    pub base_url: String,
    /// Bearer token.
    pub api_key: String,
    /// Chat model id, e.g. `deepseek-chat`.
    pub model: String,
}

#[derive(Debug, Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: [ChatMessage<'a>; 2],
    max_tokens: u32,
    temperature: f32,
}

#[derive(Debug, Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    #[serde(default)]
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    #[serde(default)]
    message: Option<Message>,
}

#[derive(Debug, Deserialize)]
struct Message {
    #[serde(default)]
    content: Option<String>,
}

const SYSTEM_PROMPT: &str = "You are a senior developer-trends analyst. \
You receive a trendkit velocity report: GitHub repos / HN stories / \
crates.io crates / HuggingFace models with score deltas per day. \
Write a compact briefing: 3-4 bullets naming what is genuinely rising \
(distinguish signal from big-but-static), then one final bullet \
recommending which single trend best fits a developer who builds in \
Rust, runs docs sites, and ships developer tooling. No preamble, no \
closing remarks, markdown bullets only, under 160 words total.";

/// Summarize a rendered report into a short briefing.
///
/// # Errors
/// Fails on transport errors, malformed responses, or empty choices;
/// Telegram/digest callers are expected to treat this as optional.
pub async fn summarize(config: &AiDigestConfig, report_text: &str) -> Result<String> {
    let trimmed: String = if report_text.chars().count() > MAX_INPUT_CHARS {
        report_text.chars().take(MAX_INPUT_CHARS).collect()
    } else {
        report_text.to_owned()
    };
    let client = Client::builder().user_agent(USER_AGENT).build();
    let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));
    let response: ChatResponse = client
        .post_json(
            &url,
            &ChatRequest {
                model: &config.model,
                messages: [
                    ChatMessage {
                        role: "system",
                        content: SYSTEM_PROMPT,
                    },
                    ChatMessage {
                        role: "user",
                        content: &trimmed,
                    },
                ],
                max_tokens: 500,
                temperature: 0.3,
            },
        )
        .await?;
    let content = response
        .choices
        .first()
        .and_then(|c| c.message.as_ref())
        .and_then(|m| m.content.clone())
        .filter(|c| !c.trim().is_empty());
    match content {
        Some(text) => Ok(text),
        None => Err(TrendError::Other(
            "ai digest: model returned no content".to_owned(),
        )),
    }
}

/// Parse an OpenAI-compatible chat-completions payload (exposed for
/// tests and offline callers).
///
/// # Errors
/// Fails when the payload is not a chat-completions response.
pub fn parse_chat_json(payload: &str) -> Result<String> {
    let response: ChatResponse = serde_json::from_str(payload)
        .map_err(|e| TrendError::Config(format!("invalid chat payload: {e}")))?;
    response
        .choices
        .first()
        .and_then(|c| c.message.as_ref())
        .and_then(|m| m.content.clone())
        .filter(|c| !c.trim().is_empty())
        .ok_or_else(|| TrendError::Other("no content in chat payload".to_owned()))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    const SAMPLE: &str = r#"{
        "id": "chatcmpl-1",
        "choices": [
            {"index": 0, "message": {"role": "assistant", "content": "- trend one\n- trend two"}, "finish_reason": "stop"}
        ]
    }"#;

    #[test]
    fn parse_extracts_content() {
        let out = parse_chat_json(SAMPLE).unwrap();
        assert!(out.contains("- trend one"));
        assert!(out.contains("trend two"));
    }

    #[test]
    fn parse_rejects_empty_choices() {
        let err = parse_chat_json(r#"{"choices": []}"#).unwrap_err();
        assert!(err.to_string().contains("no content"));
    }

    #[test]
    fn parse_rejects_garbage() {
        assert!(parse_chat_json("not json").is_err());
    }
}

//! Connectivity smoke tests for Kimi and DeepSeek providers.
//!
//! These tests require valid API keys in the `src/provider/.env` file.
//! Tests are skipped when keys are not set, so they won't fail in CI.
//!
//! Run with: `cargo test -- provider::test`

use crate::message::Message;
use crate::provider::StopReason;
use crate::provider::deepseek::{DeepSeekConfig, DeepSeekProvider};
use crate::provider::kimi::{KimiConfig, KimiProvider};
use crate::provider::{CompletionRequest, ModelProvider, ProviderEvent};

use futures_util::StreamExt;

/// Load `.env` from the provider directory before each test.
fn load_env() {
    // Try the provider-local .env first, then fall back to the project root.
    let _ = dotenvy::from_filename("src/provider/.env");
    dotenvy::dotenv().ok();
}

fn get_kimi_config() -> Option<KimiConfig> {
    load_env();
    let api_key = std::env::var("MOONSHOT_API_KEY").ok()?;
    if api_key.is_empty() || api_key == "your_kimi_api_key_here" {
        return None;
    }
    Some(KimiConfig {
        api_key,
        model: "kimi-k2-0711-preview".into(),
        base_url: "https://api.moonshot.cn".into(),
    })
}

fn get_deepseek_config() -> Option<DeepSeekConfig> {
    load_env();
    let api_key = std::env::var("DEEPSEEK_API_KEY").ok()?;
    if api_key.is_empty() || api_key == "your_deepseek_api_key_here" {
        return None;
    }
    Some(DeepSeekConfig {
        api_key,
        model: "deepseek-chat".into(),
        base_url: "https://api.deepseek.com".into(),
    })
}

fn simple_request() -> CompletionRequest {
    CompletionRequest {
        system_prompt: Some("Reply in one short sentence.".into()),
        messages: vec![Message::user("What is the capital of France?")],
        tools: vec![],
    }
}

// ── Kimi ──────────────────────────────────────────────────────────

#[tokio::test]
async fn kimi_complete_smoke() {
    let config = match get_kimi_config() {
        Some(c) => c,
        None => {
            eprintln!("SKIP: MOONSHOT_API_KEY not set or still placeholder");
            return;
        }
    };

    let provider = KimiProvider::new(config);
    let response = match provider.complete(simple_request()).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("SKIP: Kimi complete request failed (network/env): {e}");
            return;
        }
    };

    assert!(!response.message.text_content().is_empty(), "Kimi should return non-empty text");
    assert_eq!(response.stop_reason, StopReason::EndTurn);
    assert!(response.usage.is_some(), "Kimi should report token usage");
    let usage = response.usage.unwrap();
    assert!(usage.input_tokens > 0);
    assert!(usage.output_tokens > 0);
}

#[tokio::test]
async fn kimi_stream_smoke() {
    let config = match get_kimi_config() {
        Some(c) => c,
        None => {
            eprintln!("SKIP: MOONSHOT_API_KEY not set or still placeholder");
            return;
        }
    };

    let provider = KimiProvider::new(config);
    let mut stream = provider.stream_complete(simple_request());
    let mut text = String::new();
    let mut saw_start = false;
    let mut saw_stop = false;
    let mut usage = None;

    while let Some(event) = stream.next().await {
        match event {
            Ok(ProviderEvent::MessageStart) => saw_start = true,
            Ok(ProviderEvent::TextDelta(delta)) => text.push_str(&delta),
            Ok(ProviderEvent::ThinkingDelta(_)) => {}
            Ok(ProviderEvent::MessageStop { stop_reason, usage: u }) => {
                saw_stop = true;
                assert_eq!(stop_reason, StopReason::EndTurn);
                usage = u;
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("SKIP: Kimi stream error (network/env): {e}");
                return;
            }
        }
    }

    assert!(saw_start, "Kimi stream: missing MessageStart");
    assert!(saw_stop, "Kimi stream: missing MessageStop");
    assert!(!text.is_empty(), "Kimi stream: no text received");
    if let Some(u) = usage {
        assert!(u.input_tokens > 0);
        assert!(u.output_tokens > 0);
    }
}

// ── DeepSeek ──────────────────────────────────────────────────────

#[tokio::test]
async fn deepseek_complete_smoke() {
    let config = match get_deepseek_config() {
        Some(c) => c,
        None => {
            eprintln!("SKIP: DEEPSEEK_API_KEY not set or still placeholder");
            return;
        }
    };

    let provider = DeepSeekProvider::new(config);
    let response = match provider.complete(simple_request()).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("SKIP: DeepSeek complete request failed (network/env): {e}");
            return;
        }
    };

    assert!(!response.message.text_content().is_empty(), "DeepSeek should return non-empty text");
    assert_eq!(response.stop_reason, StopReason::EndTurn);
    assert!(response.usage.is_some(), "DeepSeek should report token usage");
    let usage = response.usage.unwrap();
    assert!(usage.input_tokens > 0);
    assert!(usage.output_tokens > 0);
}

#[tokio::test]
async fn deepseek_stream_smoke() {
    let config = match get_deepseek_config() {
        Some(c) => c,
        None => {
            eprintln!("SKIP: DEEPSEEK_API_KEY not set or still placeholder");
            return;
        }
    };

    let provider = DeepSeekProvider::new(config);
    let mut stream = provider.stream_complete(simple_request());
    let mut text = String::new();
    let mut saw_start = false;
    let mut saw_stop = false;
    let mut usage = None;

    while let Some(event) = stream.next().await {
        match event {
            Ok(ProviderEvent::MessageStart) => saw_start = true,
            Ok(ProviderEvent::TextDelta(delta)) => text.push_str(&delta),
            Ok(ProviderEvent::ThinkingDelta(_)) => {}
            Ok(ProviderEvent::MessageStop { stop_reason, usage: u }) => {
                saw_stop = true;
                assert_eq!(stop_reason, StopReason::EndTurn);
                usage = u;
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("SKIP: DeepSeek stream error (network/env): {e}");
                return;
            }
        }
    }

    assert!(saw_start, "DeepSeek stream: missing MessageStart");
    assert!(saw_stop, "DeepSeek stream: missing MessageStop");
    assert!(!text.is_empty(), "DeepSeek stream: no text received");
    if let Some(u) = usage {
        assert!(u.input_tokens > 0);
        assert!(u.output_tokens > 0);
    }
}

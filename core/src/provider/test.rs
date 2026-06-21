//! Connectivity smoke tests for the DeepSeek provider.
//!
//! These tests require valid API keys in the `src/provider/.env` file.
//! Tests are skipped when keys are not set, so they won't fail in CI.
//!
//! Run with: `cargo test -- provider::test`

use crate::message::Message;
use crate::provider::StopReason;
use crate::provider::deepseek::{
    DeepSeekChatOptions, DeepSeekConfig, DeepSeekFimRequest, DeepSeekProvider,
    DeepSeekResponseFormat,
};
use crate::provider::{CompletionRequest, ModelProvider, ProviderEvent};

use futures_util::StreamExt;

/// Load `.env` from the provider directory before each test.
fn load_env() {
    // Try the provider-local .env first, then fall back to the project root.
    let _ = dotenvy::from_filename("src/provider/.env");
    dotenvy::dotenv().ok();
}

fn get_deepseek_config() -> Option<DeepSeekConfig> {
    load_env();
    let api_key = deepseek_test_key()?;
    if api_key.is_empty() || api_key == "your_deepseek_api_key_here" {
        return None;
    }
    Some(DeepSeekConfig {
        api_key,
        model: "deepseek-chat".into(),
        base_url: "https://api.deepseek.com".into(),
    })
}

fn deepseek_test_key() -> Option<String> {
    std::env::var("DEEPSEEK_TEST_KEY").ok()
}

fn simple_request() -> CompletionRequest {
    CompletionRequest {
        system_prompt: Some("Reply in one short sentence.".into()),
        system_prompt_blocks: None,
        messages: vec![Message::user("What is the capital of France?")],
        tools: vec![],
        model_hint: None,
        max_tokens: None,
    }
}

// ── DeepSeek ──────────────────────────────────────────────────────

#[tokio::test]
async fn deepseek_complete_smoke() {
    let config = match get_deepseek_config() {
        Some(c) => c,
        None => {
            eprintln!("SKIP: DEEPSEEK_TEST_KEY not set");
            return;
        }
    };

    let provider = DeepSeekProvider::new(config);
    let response = provider.complete(simple_request()).await.unwrap();

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
            eprintln!("SKIP: DEEPSEEK_TEST_KEY not set");
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
            Ok(ProviderEvent::MessageStop { stop_reason, usage: u, .. }) => {
                saw_stop = true;
                assert_eq!(stop_reason, StopReason::EndTurn);
                usage = u;
            }
            Ok(_) => {}
            Err(e) => panic!("DeepSeek stream error: {e}"),
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

#[tokio::test]
async fn deepseek_json_output_real_api() {
    let config = match get_deepseek_config() {
        Some(c) => c,
        None => {
            eprintln!("SKIP: DEEPSEEK_TEST_KEY not set");
            return;
        }
    };

    let provider = DeepSeekProvider::new(config);
    let request = CompletionRequest {
        system_prompt: Some(
            "Return only a valid compact JSON object with an ok boolean field.".into(),
        ),
        system_prompt_blocks: None,
        messages: vec![Message::user("Return ok true.")],
        tools: vec![],
        model_hint: None,
        max_tokens: None,
    };
    let response = provider
        .complete_with_options(
            request,
            DeepSeekChatOptions {
                response_format: Some(DeepSeekResponseFormat::JsonObject),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let value: serde_json::Value = serde_json::from_str(&response.message.text_content()).unwrap();
    assert_eq!(value.get("ok").and_then(serde_json::Value::as_bool), Some(true));
    assert!(response.usage.is_some(), "DeepSeek JSON output should report usage");
}

#[tokio::test]
async fn deepseek_models_and_balance_real_api() {
    let config = match get_deepseek_config() {
        Some(c) => c,
        None => {
            eprintln!("SKIP: DEEPSEEK_TEST_KEY not set");
            return;
        }
    };

    let provider = DeepSeekProvider::new(config);
    let models = provider.list_models().await.unwrap();
    assert!(!models.data.is_empty(), "DeepSeek model list should not be empty");
    assert!(
        models.data.iter().any(|model| model.id.contains("deepseek")),
        "DeepSeek model list should include a DeepSeek model"
    );

    let balance = provider.balance().await.unwrap();
    assert!(
        !balance.balance_infos.is_empty() || !balance.is_available,
        "DeepSeek balance response should include balances unless unavailable"
    );
}

#[tokio::test]
async fn deepseek_fim_real_api() {
    let config = match get_deepseek_config() {
        Some(c) => c,
        None => {
            eprintln!("SKIP: DEEPSEEK_TEST_KEY not set");
            return;
        }
    };

    let provider = DeepSeekProvider::new(config);
    let response = provider
        .fim_complete(DeepSeekFimRequest {
            prompt: "fn answer() -> i32 {".into(),
            suffix: Some("}".into()),
            max_tokens: Some(24),
            temperature: Some(0.0),
            ..Default::default()
        })
        .await
        .unwrap();

    assert!(!response.choices.is_empty(), "DeepSeek FIM should return choices");
    assert!(!response.choices[0].text.trim().is_empty(), "DeepSeek FIM should return text");
    assert!(response.usage.is_some(), "DeepSeek FIM should report usage");
}

#[tokio::test]
async fn deepseek_prefix_real_api() {
    let config = match get_deepseek_config() {
        Some(c) => c,
        None => {
            eprintln!("SKIP: DEEPSEEK_TEST_KEY not set");
            return;
        }
    };

    let provider = DeepSeekProvider::new(config);
    let request = CompletionRequest {
        system_prompt: Some(
            "Continue the assistant prefix with exactly the integer literal 42. Do not add punctuation."
                .into(),
        ),
        system_prompt_blocks: None,
        messages: vec![Message::user("Complete this Rust return value.")],
        tools: vec![],
        model_hint: None,
            max_tokens: None,
    };

    let response = provider
        .complete_with_options(
            request,
            DeepSeekChatOptions {
                prefix: Some("return ".into()),
                stop: Some(vec![";".into(), "\n".into()]),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    assert_eq!(response.message.text_content().trim(), "42");
    let usage = response.usage.expect("DeepSeek prefix completion should report usage");
    assert!(usage.output_tokens > 0, "DeepSeek prefix completion should emit tokens");
}

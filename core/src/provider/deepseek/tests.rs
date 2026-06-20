use super::*;
use crate::error::{AgentError, ProviderError};
use crate::message::Message;
use crate::provider::{
    CompletionRequest, ModelHint, ModelProvider, ProviderEvent, StopReason, TokenUsage,
};
use futures_util::StreamExt;
use serde_json::json;
use wiremock::matchers::{body_partial_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn test_config(base_url: String) -> DeepSeekConfig {
    DeepSeekConfig { api_key: "test-deepseek-key".into(), model: "deepseek-chat".into(), base_url }
}

#[test]
fn default_base_url() {
    let config = DeepSeekConfig {
        api_key: "x".into(),
        model: "deepseek-chat".into(),
        base_url: "https://api.deepseek.com".into(),
    };
    let provider = DeepSeekProvider::new(config);
    assert_eq!(provider.model, "deepseek-chat");
}

#[tokio::test]
async fn completes_chat_request() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("authorization", "Bearer test-deepseek-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-test",
            "object": "chat.completion",
            "created": 1,
            "model": "deepseek-chat",
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "Hello from DeepSeek!" },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 10, "completion_tokens": 3, "total_tokens": 13 }
        })))
        .mount(&server)
        .await;

    let provider = DeepSeekProvider::new(test_config(server.uri()));
    let request = CompletionRequest {
        system_prompt: None,
        system_prompt_blocks: None,
        messages: vec![Message::user("Hi")],
        tools: vec![],
        model_hint: None,
    };

    let response = provider.complete(request).await.unwrap();
    assert_eq!(response.message.text_content(), "Hello from DeepSeek!");
    assert_eq!(response.stop_reason, StopReason::EndTurn);
    assert_eq!(
        response.usage,
        Some(TokenUsage {
            input_tokens: 10,
            output_tokens: 3,
            total_tokens: Some(13),
            prompt_cache_hit_tokens: None,
            prompt_cache_miss_tokens: None,
            reasoning_tokens: None,
        })
    );
}

#[tokio::test]
async fn thinking_hint_enables_deepseek_thinking_request() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(body_partial_json(json!({
            "model": "deepseek-chat",
            "thinking": { "type": "enabled" }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-test",
            "object": "chat.completion",
            "created": 1,
            "model": "deepseek-chat",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "reasoning_content": "I should answer briefly.",
                    "content": "Hello!"
                },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15 }
        })))
        .mount(&server)
        .await;

    let provider = DeepSeekProvider::new(test_config(server.uri()));
    let request = CompletionRequest {
        system_prompt: None,
        system_prompt_blocks: None,
        messages: vec![Message::user("Hi")],
        tools: vec![],
        model_hint: Some(ModelHint::Thinking),
    };

    let response = provider.complete(request).await.unwrap();
    assert_eq!(response.message.thinking_content(), "I should answer briefly.");
    assert_eq!(response.message.text_content(), "Hello!");
}

#[tokio::test]
async fn execution_hint_does_not_enable_deepseek_thinking_request() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-test",
            "object": "chat.completion",
            "created": 1,
            "model": "deepseek-chat",
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "Hello!" },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15 }
        })))
        .mount(&server)
        .await;

    let provider = DeepSeekProvider::new(test_config(server.uri()));
    let request = CompletionRequest {
        system_prompt: None,
        system_prompt_blocks: None,
        messages: vec![Message::user("Hi")],
        tools: vec![],
        model_hint: Some(ModelHint::Execution),
    };

    provider.complete(request).await.unwrap();
    let requests = server.received_requests().await.unwrap();
    let body: serde_json::Value = requests[0].body_json().unwrap();
    assert!(body.get("thinking").is_none(), "execution requests must not enable thinking");
}

#[tokio::test]
async fn parses_deepseek_usage_details() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-test",
            "object": "chat.completion",
            "created": 1,
            "model": "deepseek-chat",
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "Hello!" },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 20,
                "completion_tokens": 7,
                "total_tokens": 27,
                "prompt_cache_hit_tokens": 12,
                "prompt_cache_miss_tokens": 8,
                "completion_tokens_details": {
                    "reasoning_tokens": 4
                }
            }
        })))
        .mount(&server)
        .await;

    let provider = DeepSeekProvider::new(test_config(server.uri()));
    let request = CompletionRequest {
        system_prompt: None,
        system_prompt_blocks: None,
        messages: vec![Message::user("Hi")],
        tools: vec![],
        model_hint: None,
    };

    let response = provider.complete(request).await.unwrap();
    assert_eq!(
        response.usage,
        Some(TokenUsage {
            input_tokens: 20,
            output_tokens: 7,
            total_tokens: Some(27),
            prompt_cache_hit_tokens: Some(12),
            prompt_cache_miss_tokens: Some(8),
            reasoning_tokens: Some(4),
        })
    );
}

#[tokio::test]
async fn maps_deepseek_error_codes_to_actionable_messages_and_retryability() {
    let cases = [
        (400, "Bad Request", false),
        (401, "Authentication Fails", false),
        (402, "Insufficient Balance", false),
        (422, "Invalid Parameters", false),
        (429, "Rate Limit Reached", true),
        (500, "Server Error", true),
        (503, "Server Overloaded", true),
    ];

    for (status, message, retryable) in cases {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(status).set_body_json(json!({
                "error": { "message": message, "type": "invalid_request_error" }
            })))
            .mount(&server)
            .await;

        let provider = DeepSeekProvider::new(test_config(server.uri()));
        let request = CompletionRequest {
            system_prompt: None,
            system_prompt_blocks: None,
            messages: vec![Message::user("Hi")],
            tools: vec![],
            model_hint: None,
        };

        let err = provider.complete(request).await.unwrap_err();
        assert_eq!(err.is_retryable(), retryable, "status {status}");
        let AgentError::Provider(ProviderError::Http { status: got_status, message: got_message }) =
            err
        else {
            panic!("expected provider HTTP error for status {status}");
        };
        assert_eq!(got_status, status);
        assert!(
            got_message.contains(message),
            "mapped message should include provider message, got: {got_message}"
        );
    }
}

#[tokio::test]
async fn complete_with_json_output_sets_response_format() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(body_partial_json(json!({
            "response_format": { "type": "json_object" }
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "chatcmpl-test",
            "object": "chat.completion",
            "created": 1,
            "model": "deepseek-chat",
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": "{\"ok\":true}" },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 10, "completion_tokens": 3, "total_tokens": 13 }
        })))
        .mount(&server)
        .await;

    let provider = DeepSeekProvider::new(test_config(server.uri()));
    let request = CompletionRequest {
        system_prompt: Some("Return json.".into()),
        system_prompt_blocks: None,
        messages: vec![Message::user("Return {\"ok\": true} as json")],
        tools: vec![],
        model_hint: None,
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
    assert_eq!(response.message.text_content(), "{\"ok\":true}");
}

#[tokio::test]
async fn complete_with_prefix_uses_beta_chat_and_prefix_message() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
            .and(path("/beta/chat/completions"))
            .and(body_partial_json(json!({
                "messages": [
                    { "role": "user", "content": "Write quick sort" },
                    { "role": "assistant", "content": "```python\n", "prefix": true }
                ],
                "stop": ["```"]
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": "chatcmpl-test",
                "object": "chat.completion",
                "created": 1,
                "model": "deepseek-chat",
                "choices": [{
                    "index": 0,
                    "message": { "role": "assistant", "content": "def quick_sort(xs):\n    return xs\n" },
                    "finish_reason": "stop"
                }],
                "usage": { "prompt_tokens": 12, "completion_tokens": 8, "total_tokens": 20 }
            })))
            .mount(&server)
            .await;

    let provider = DeepSeekProvider::new(test_config(server.uri()));
    let request = CompletionRequest {
        system_prompt: None,
        system_prompt_blocks: None,
        messages: vec![Message::user("Write quick sort")],
        tools: vec![],
        model_hint: None,
    };

    let response = provider
        .complete_with_options(
            request,
            DeepSeekChatOptions {
                prefix: Some("```python\n".into()),
                stop: Some(vec!["```".into()]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(response.message.text_content().starts_with("def quick_sort"));
}

#[tokio::test]
async fn fim_completion_posts_to_beta_completions() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/beta/completions"))
        .and(body_partial_json(json!({
            "model": "deepseek-chat",
            "prompt": "fn main() {",
            "suffix": "}",
            "max_tokens": 16
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "cmpl-test",
            "object": "text_completion",
            "created": 1,
            "model": "deepseek-chat",
            "choices": [{
                "index": 0,
                "text": "\n    println!(\"hi\");\n",
                "finish_reason": "stop",
                "logprobs": null
            }],
            "usage": {
                "prompt_tokens": 5,
                "completion_tokens": 4,
                "prompt_cache_hit_tokens": 2,
                "prompt_cache_miss_tokens": 3,
                "total_tokens": 9
            }
        })))
        .mount(&server)
        .await;

    let provider = DeepSeekProvider::new(test_config(server.uri()));
    let response = provider
        .fim_complete(DeepSeekFimRequest {
            prompt: "fn main() {".into(),
            suffix: Some("}".into()),
            max_tokens: Some(16),
            ..Default::default()
        })
        .await
        .unwrap();

    assert_eq!(response.choices[0].text, "\n    println!(\"hi\");\n");
    assert_eq!(
        response.usage,
        Some(TokenUsage {
            input_tokens: 5,
            output_tokens: 4,
            total_tokens: Some(9),
            prompt_cache_hit_tokens: Some(2),
            prompt_cache_miss_tokens: Some(3),
            reasoning_tokens: None,
        })
    );
}

#[tokio::test]
async fn list_models_and_balance_use_native_get_endpoints() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "object": "list",
            "data": [
                { "id": "deepseek-v4-flash", "object": "model", "owned_by": "deepseek" },
                { "id": "deepseek-v4-pro", "object": "model", "owned_by": "deepseek" }
            ]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/user/balance"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "is_available": true,
            "balance_infos": [
                {
                    "currency": "CNY",
                    "total_balance": "110.00",
                    "granted_balance": "10.00",
                    "topped_up_balance": "100.00"
                }
            ]
        })))
        .mount(&server)
        .await;

    let provider = DeepSeekProvider::new(test_config(server.uri()));
    let models = provider.list_models().await.unwrap();
    assert_eq!(models.data[0].id, "deepseek-v4-flash");

    let balance = provider.balance().await.unwrap();
    assert!(balance.is_available);
    assert_eq!(balance.balance_infos[0].currency, "CNY");
}

#[tokio::test]
async fn streams_chat_response() {
    let server = MockServer::start().await;
    let body = "data: {\"id\":\"chatcmpl-test\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"deepseek-chat\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hello\"},\"finish_reason\":null}]}\n\n\
            data: {\"id\":\"chatcmpl-test\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"deepseek-chat\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"!\"},\"finish_reason\":null}]}\n\n\
            data: {\"id\":\"chatcmpl-test\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"deepseek-chat\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n\
            data: [DONE]\n\n";
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("authorization", "Bearer test-deepseek-key"))
        .and(body_partial_json(json!({
            "stream": true,
            "stream_options": { "include_usage": true }
        })))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(body),
        )
        .mount(&server)
        .await;

    let provider = DeepSeekProvider::new(test_config(server.uri()));
    let request = CompletionRequest {
        system_prompt: None,
        system_prompt_blocks: None,
        messages: vec![Message::user("Hi")],
        tools: vec![],
        model_hint: None,
    };

    let events: Vec<_> = provider.stream_complete(request).collect().await;
    let mut text = String::new();
    let mut saw_start = false;
    let mut saw_stop = false;
    for event in events {
        match event.unwrap() {
            ProviderEvent::MessageStart => saw_start = true,
            ProviderEvent::TextDelta(delta) => text.push_str(&delta),
            ProviderEvent::MessageStop { stop_reason, .. } => {
                saw_stop = true;
                assert_eq!(stop_reason, StopReason::EndTurn);
            }
            _ => panic!("unexpected event"),
        }
    }
    assert!(saw_start);
    assert!(saw_stop);
    assert_eq!(text, "Hello!");
}

#[tokio::test]
async fn streams_chat_response_ignores_null_usage_until_final_usage() {
    let server = MockServer::start().await;
    let body = "data: {\"id\":\"chatcmpl-test\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"deepseek-chat\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hello\"},\"finish_reason\":null}],\"usage\":null}\n\n\
            data: {\"id\":\"chatcmpl-test\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"deepseek-chat\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"!\"},\"finish_reason\":null}],\"usage\":null}\n\n\
            data: {\"id\":\"chatcmpl-test\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"deepseek-chat\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":4,\"completion_tokens\":2,\"total_tokens\":6}}\n\n\
            data: [DONE]\n\n";
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("authorization", "Bearer test-deepseek-key"))
        .and(body_partial_json(json!({
            "stream": true,
            "stream_options": { "include_usage": true }
        })))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(body),
        )
        .mount(&server)
        .await;

    let provider = DeepSeekProvider::new(test_config(server.uri()));
    let request = CompletionRequest {
        system_prompt: None,
        system_prompt_blocks: None,
        messages: vec![Message::user("Hi")],
        tools: vec![],
        model_hint: None,
    };

    let events: Vec<_> = provider.stream_complete(request).collect().await;
    let mut text = String::new();
    let mut usage = None;
    for event in events {
        match event.unwrap() {
            ProviderEvent::TextDelta(delta) => text.push_str(&delta),
            ProviderEvent::MessageStop { usage: final_usage, .. } => usage = final_usage,
            _ => {}
        }
    }

    assert_eq!(text, "Hello!");
    assert_eq!(
        usage,
        Some(TokenUsage {
            input_tokens: 4,
            output_tokens: 2,
            total_tokens: Some(6),
            prompt_cache_hit_tokens: None,
            prompt_cache_miss_tokens: None,
            reasoning_tokens: None,
        })
    );
}

#[tokio::test]
async fn streams_chat_response_allows_null_usage_without_final_usage() {
    let server = MockServer::start().await;
    let body = "data: {\"id\":\"chatcmpl-test\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"deepseek-chat\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"Hello\"},\"finish_reason\":null}],\"usage\":null}\n\n\
            data: {\"id\":\"chatcmpl-test\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"deepseek-chat\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":null}\n\n\
            data: [DONE]\n\n";
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("authorization", "Bearer test-deepseek-key"))
        .and(body_partial_json(json!({
            "stream": true,
            "stream_options": { "include_usage": true }
        })))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(body),
        )
        .mount(&server)
        .await;

    let provider = DeepSeekProvider::new(test_config(server.uri()));
    let request = CompletionRequest {
        system_prompt: None,
        system_prompt_blocks: None,
        messages: vec![Message::user("Hi")],
        tools: vec![],
        model_hint: None,
    };

    let events: Vec<_> = provider.stream_complete(request).collect().await;
    let mut text = String::new();
    let mut usage = Some(TokenUsage {
        input_tokens: 999,
        output_tokens: 999,
        total_tokens: None,
        prompt_cache_hit_tokens: None,
        prompt_cache_miss_tokens: None,
        reasoning_tokens: None,
    });
    for event in events {
        match event.unwrap() {
            ProviderEvent::TextDelta(delta) => text.push_str(&delta),
            ProviderEvent::MessageStop { usage: final_usage, .. } => usage = final_usage,
            _ => {}
        }
    }

    assert_eq!(text, "Hello");
    assert_eq!(usage, None);
}

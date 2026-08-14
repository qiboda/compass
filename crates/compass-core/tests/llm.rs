//! Requirement acceptance tests for `compass_core::llm` (epic #243 Batch 4,
//! ref #247, plan Todo 2).
//!
//! RED phase: the `llm` module does not exist yet — this file fails to
//! compile until Todo 2 lands (missing symbols `compass_core::llm::*`).
//! Contract (plan): `LlmConfig{base_url,api_key,model}` (Deserialize),
//! `LlmError{EmptyConfig,Network,Http{status,body},NoContent,InvalidJson}`,
//! `LlmClient::new(config) -> Result<Self, LlmError>` (validates base_url and
//! model non-empty; api_key deliberately unchecked), and
//! `chat_json(&self, system, user) -> Result<serde_json::Value, LlmError>`
//! POSTing `{base_url}/chat/completions` with Bearer auth.

use compass_core::llm::{LlmClient, LlmConfig, LlmError};
use httpmock::Method::POST;
use httpmock::MockServer;
use serde_json::json;

/// Client pointed at an httpmock server (API root `/v1`).
fn client_at(server: &MockServer) -> LlmClient {
    LlmClient::new(LlmConfig {
        base_url: format!("{}/v1", server.base_url()),
        api_key: "sk-test".to_string(),
        model: "gpt-test".to_string(),
    })
    .expect("valid base_url/model must construct")
}

/// Mock a 200 chat-completions response whose `content` string holds `raw`.
fn mock_ok_content<'a>(server: &'a MockServer, raw: &str) -> httpmock::Mock<'a> {
    server.mock(|when, then| {
        when.method(POST).path("/v1/chat/completions");
        then.status(200).json_body(json!({
            "choices": [{"message": {"content": raw}}]
        }));
    })
}

#[tokio::test]
async fn chat_json_returns_parsed_content_on_success() {
    let server = MockServer::start();
    let mock = mock_ok_content(&server, "{\"foo\": 1}");
    let client = client_at(&server);

    let value = client.chat_json("system", "user").await.expect("ok");

    assert_eq!(value, json!({"foo": 1}));
    mock.assert();
}

#[tokio::test]
async fn chat_json_sends_model_and_messages_in_request_body() {
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/chat/completions")
            .body_includes("\"model\"")
            .body_includes("gpt-test")
            .body_includes("选股条件生成助手")
            .body_includes("最近5天每天涨超3%");
        then.status(200)
            .json_body(json!({"choices": [{"message": {"content": "{}"}}]}));
    });
    let client = client_at(&server);

    let _ = client
        .chat_json("你是A股选股条件生成助手", "最近5天每天涨超3%")
        .await
        .expect("ok");

    mock.assert();
}

#[tokio::test]
async fn chat_json_errors_on_invalid_json_content() {
    let server = MockServer::start();
    let _mock = mock_ok_content(&server, "this is not json");
    let client = client_at(&server);

    let err = client
        .chat_json("s", "u")
        .await
        .expect_err("invalid content must error");
    assert!(
        matches!(err, LlmError::InvalidJson(_)),
        "expected InvalidJson, got {err:?}"
    );
}

#[tokio::test]
async fn chat_json_errors_on_http_500() {
    let server = MockServer::start();
    let _mock = server.mock(|when, then| {
        when.method(POST).path("/v1/chat/completions");
        then.status(500).body("internal server error");
    });
    let client = client_at(&server);

    let err = client
        .chat_json("s", "u")
        .await
        .expect_err("5xx must error");
    match err {
        LlmError::Http { status, body } => {
            assert_eq!(status, 500);
            assert!(body.contains("internal server error"));
        }
        other => panic!("expected Http{{status:500,..}}, got {other:?}"),
    }
}

#[tokio::test]
async fn chat_json_errors_on_empty_choices() {
    let server = MockServer::start();
    let _mock = server.mock(|when, then| {
        when.method(POST).path("/v1/chat/completions");
        then.status(200).json_body(json!({"choices": []}));
    });
    let client = client_at(&server);

    let err = client
        .chat_json("s", "u")
        .await
        .expect_err("empty choices must error");
    assert!(
        matches!(err, LlmError::NoContent),
        "expected NoContent, got {err:?}"
    );
}

#[tokio::test]
async fn chat_json_errors_on_missing_content_field() {
    let server = MockServer::start();
    let _mock = server.mock(|when, then| {
        when.method(POST).path("/v1/chat/completions");
        then.status(200)
            .json_body(json!({"choices": [{"message": {}}]}));
    });
    let client = client_at(&server);

    let err = client
        .chat_json("s", "u")
        .await
        .expect_err("missing content must error");
    assert!(
        matches!(err, LlmError::NoContent),
        "expected NoContent, got {err:?}"
    );
}

#[tokio::test]
async fn chat_json_errors_on_unmatched_path_404() {
    let server = MockServer::start();
    let client = client_at(&server);

    let err = client
        .chat_json("s", "u")
        .await
        .expect_err("404 must error");
    match err {
        LlmError::Http { status, .. } => assert_eq!(status, 404),
        other => panic!("expected Http{{status:404,..}}, got {other:?}"),
    }
}

#[tokio::test]
async fn chat_json_errors_on_malformed_base_url() {
    let client = LlmClient::new(LlmConfig {
        base_url: "not a url".to_string(),
        api_key: "sk-test".to_string(),
        model: "gpt-test".to_string(),
    })
    .expect("non-empty base_url must construct");

    let err = client
        .chat_json("s", "u")
        .await
        .expect_err("bad URL must error");
    assert!(
        matches!(err, LlmError::Network(_)),
        "expected Network, got {err:?}"
    );
}

#[test]
fn new_rejects_empty_base_url() {
    let res = LlmClient::new(LlmConfig {
        base_url: String::new(),
        api_key: "sk-test".to_string(),
        model: "gpt-test".to_string(),
    });
    assert!(
        matches!(res, Err(LlmError::EmptyConfig(_))),
        "empty base_url must be EmptyConfig, got {res:?}"
    );
}

#[test]
fn new_rejects_empty_model() {
    let res = LlmClient::new(LlmConfig {
        base_url: "https://api.openai.com/v1".to_string(),
        api_key: "sk-test".to_string(),
        model: String::new(),
    });
    assert!(
        matches!(res, Err(LlmError::EmptyConfig(_))),
        "empty model must be EmptyConfig, got {res:?}"
    );
}

#[test]
fn new_accepts_empty_api_key() {
    // Plan decision: `new` only validates base_url/model; api_key emptiness
    // is the caller's concern (backend checks is_configured).
    let res = LlmClient::new(LlmConfig {
        base_url: "https://api.openai.com/v1".to_string(),
        api_key: String::new(),
        model: "gpt-test".to_string(),
    });
    assert!(res.is_ok(), "empty api_key must still construct: {res:?}");
}

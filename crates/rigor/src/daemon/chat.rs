use axum::extract::State;
use axum::response::IntoResponse;
use axum::Json;
use uuid::Uuid;

use super::context::build_epistemic_context;
use super::ws::DaemonEvent;
use super::SharedState;

#[derive(serde::Deserialize)]
pub struct ChatRequest {
    pub message: String,
    pub history: Vec<ChatMessage>,
}

#[derive(serde::Deserialize, serde::Serialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

pub async fn chat_handler(
    State(state): State<SharedState>,
    Json(body): Json<ChatRequest>,
) -> impl IntoResponse {
    let chat_id = Uuid::new_v4().to_string();

    let (target_api, api_key, epistemic_context, http_client, event_tx) = {
        let st = state.lock().unwrap();
        let ctx = build_epistemic_context(&st.config, &st.graph);
        (
            st.target_api.clone(),
            st.api_key.clone(),
            ctx,
            st.http_client.clone(),
            st.event_tx.clone(),
        )
    };

    let api_key = match api_key {
        Some(k) => k,
        None => {
            return Json(serde_json::json!({
                "error": "No API key available. Set ANTHROPIC_API_KEY or start Claude Code through the proxy first."
            })).into_response();
        }
    };

    eprintln!(
        "rigor chat: sending to {}/v1/messages, key_prefix={}, key_len={}",
        target_api,
        &api_key[..api_key.len().min(8)],
        api_key.len()
    );

    let mut messages: Vec<serde_json::Value> = body
        .history
        .iter()
        .map(|m| serde_json::json!({"role": m.role, "content": m.content}))
        .collect();
    messages.push(serde_json::json!({"role": "user", "content": body.message}));

    let api_body = serde_json::json!({
        "model": "claude-sonnet-4-6",
        "max_tokens": 4096,
        "system": epistemic_context,
        "messages": messages,
    });

    // Use the captured key — it may be an x-api-key or an OAuth Bearer token.
    // Try both headers to maximize compatibility.
    let mut req = http_client
        .post(format!("{}/v1/messages", target_api))
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json");

    if api_key.starts_with("sk-") {
        req = req.header("x-api-key", &api_key);
    } else {
        // OAuth token or other bearer format
        req = req.header("authorization", format!("Bearer {}", api_key));
    }

    let response = match req.json(&api_body).send().await {
        Ok(r) => r,
        Err(e) => {
            return Json(serde_json::json!({"error": format!("API request failed: {}", e)}))
                .into_response();
        }
    };

    let status = response.status();
    eprintln!("rigor chat: response status={}", status);
    let response_body: serde_json::Value = match response.json().await {
        Ok(b) => b,
        Err(e) => {
            return Json(serde_json::json!({"error": format!("Failed to parse response: {}", e)}))
                .into_response();
        }
    };

    eprintln!(
        "rigor chat: response body={}",
        serde_json::to_string(&response_body).unwrap_or_default()
    );

    // Check for API errors (non-2xx status or error type in body)
    if !status.is_success() || response_body.get("type").and_then(|t| t.as_str()) == Some("error") {
        let error_msg = response_body["error"]["message"]
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                // Fall back to full error object or body for debugging
                response_body["error"]
                    .as_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| {
                        serde_json::to_string(&response_body)
                            .unwrap_or_else(|_| "Unknown API error".to_string())
                    })
            });
        return Json(serde_json::json!({
            "error": format!("Claude API error ({}): {}", status.as_u16(), error_msg)
        }))
        .into_response();
    }

    let text = response_body["content"]
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|block| block["text"].as_str())
        .unwrap_or("")
        .to_string();

    let _ = event_tx.send(DaemonEvent::ChatResponse {
        chat_id: chat_id.clone(),
        chunk: text.clone(),
        done: true,
    });

    Json(serde_json::json!({
        "chat_id": chat_id,
        "text": text,
        "model": response_body["model"].as_str().unwrap_or("unknown"),
    }))
    .into_response()
}

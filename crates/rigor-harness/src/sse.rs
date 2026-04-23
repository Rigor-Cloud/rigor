/// SSE format selector for text extraction.
pub enum SseFormat {
    Anthropic,
    OpenAI,
}

/// Split raw SSE body text into individual data-line payloads.
///
/// Strips the `data: ` prefix and skips empty lines and SSE comments (`:` prefix).
pub fn parse_sse_events(body: &str) -> Vec<String> {
    body.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with(':') {
                return None;
            }
            if let Some(data) = trimmed.strip_prefix("data: ") {
                Some(data.to_string())
            } else if let Some(data) = trimmed.strip_prefix("data:") {
                Some(data.to_string())
            } else {
                None
            }
        })
        .collect()
}

/// Extract accumulated assistant text from parsed SSE event data strings.
///
/// Mirrors the logic in `proxy.rs::extract_sse_assistant_text`:
/// - Anthropic: `content_block_delta` events with `delta.text`
/// - OpenAI: `choices[0].delta.content`
/// - Stops on `[DONE]` or `message_stop`
pub fn extract_text_from_sse(events: &[String], format: SseFormat) -> String {
    let mut text_parts = Vec::new();

    for data in events {
        if data == "[DONE]" {
            break;
        }
        let json: serde_json::Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(_) => continue,
        };

        match format {
            SseFormat::Anthropic => {
                if json.get("type").and_then(|t| t.as_str()) == Some("message_stop") {
                    break;
                }
                if json.get("type").and_then(|t| t.as_str()) == Some("content_block_delta") {
                    if let Some(text) = json
                        .get("delta")
                        .and_then(|d| d.get("text"))
                        .and_then(|t| t.as_str())
                    {
                        text_parts.push(text.to_string());
                    }
                }
            }
            SseFormat::OpenAI => {
                if let Some(content) = json
                    .get("choices")
                    .and_then(|c| c.as_array())
                    .and_then(|a| a.first())
                    .and_then(|c| c.get("delta"))
                    .and_then(|d| d.get("content"))
                    .and_then(|c| c.as_str())
                {
                    text_parts.push(content.to_string());
                }
            }
        }
    }

    text_parts.join("")
}

/// Generate the full Anthropic SSE event sequence for a given text.
///
/// Produces: message_start, content_block_start, content_block_delta (per word),
/// content_block_stop, message_delta, message_stop.
pub fn anthropic_sse_chunks(text: &str) -> Vec<String> {
    let mut chunks = Vec::new();

    chunks.push(
        r#"{"type":"message_start","message":{"id":"msg_test","type":"message","role":"assistant","model":"claude-sonnet-4-20250514","content":[],"stop_reason":null,"usage":{"input_tokens":10,"output_tokens":0}}}"#.to_string()
    );

    chunks.push(
        r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#.to_string()
    );

    let word_list: Vec<&str> = text.split_inclusive(' ').collect();
    let word_list: Vec<&str> = if word_list.is_empty() && !text.is_empty() {
        vec![text]
    } else {
        word_list
    };
    let word_count = word_list.len();

    for word in &word_list {
        let escaped = serde_json::to_string(word).unwrap_or_else(|_| format!("\"{}\"", word));
        // escaped includes surrounding quotes; strip them for embedding
        let inner = &escaped[1..escaped.len() - 1];
        chunks.push(format!(
            r#"{{"type":"content_block_delta","index":0,"delta":{{"type":"text_delta","text":"{}"}}}}"#,
            inner
        ));
    }

    chunks.push(r#"{"type":"content_block_stop","index":0}"#.to_string());

    chunks.push(format!(
        r#"{{"type":"message_delta","delta":{{"stop_reason":"end_turn"}},"usage":{{"output_tokens":{}}}}}"#,
        word_count
    ));

    chunks.push(r#"{"type":"message_stop"}"#.to_string());

    chunks
}

/// Generate the full OpenAI SSE event sequence for a given text.
///
/// Produces: role delta, content delta (per word), [DONE].
pub fn openai_sse_chunks(text: &str) -> Vec<String> {
    let mut chunks = Vec::new();

    chunks.push(
        r#"{"choices":[{"delta":{"role":"assistant"},"index":0}]}"#.to_string()
    );

    let word_list: Vec<&str> = text.split_inclusive(' ').collect();
    let word_list: Vec<&str> = if word_list.is_empty() && !text.is_empty() {
        vec![text]
    } else {
        word_list
    };

    for word in &word_list {
        let escaped = serde_json::to_string(word).unwrap_or_else(|_| format!("\"{}\"", word));
        let inner = &escaped[1..escaped.len() - 1];
        chunks.push(format!(
            r#"{{"choices":[{{"delta":{{"content":"{}"}},"index":0}}]}}"#,
            inner
        ));
    }

    chunks.push("[DONE]".to_string());

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sse_events() {
        let raw = "data: {\"type\":\"message_start\"}\n\ndata: {\"type\":\"content_block_delta\"}\n\n: comment\n\ndata: [DONE]\n\n";
        let events = parse_sse_events(raw);
        assert_eq!(events.len(), 3);
        assert_eq!(events[0], "{\"type\":\"message_start\"}");
        assert_eq!(events[2], "[DONE]");
    }

    #[test]
    fn test_extract_anthropic_text() {
        let chunks = anthropic_sse_chunks("hello world");
        let text = extract_text_from_sse(&chunks, SseFormat::Anthropic);
        assert_eq!(text, "hello world");
    }

    #[test]
    fn test_extract_openai_text() {
        let chunks = openai_sse_chunks("hello world");
        let text = extract_text_from_sse(&chunks, SseFormat::OpenAI);
        assert_eq!(text, "hello world");
    }

    #[test]
    fn test_anthropic_chunks_structure() {
        let chunks = anthropic_sse_chunks("test");
        // message_start, content_block_start, 1 delta, content_block_stop, message_delta, message_stop
        assert!(chunks.len() >= 6);
        assert!(chunks[0].contains("message_start"));
        assert!(chunks[1].contains("content_block_start"));
        assert!(chunks[2].contains("content_block_delta"));
        assert!(chunks.last().unwrap().contains("message_stop"));
    }

    #[test]
    fn test_openai_chunks_structure() {
        let chunks = openai_sse_chunks("test");
        // role delta, 1 content delta, [DONE]
        assert!(chunks.len() >= 3);
        assert!(chunks[0].contains("\"role\":\"assistant\""));
        assert_eq!(chunks.last().unwrap(), "[DONE]");
    }

    #[test]
    fn test_parse_sse_events_skip_empty_and_comments() {
        let raw = ": keep-alive\n\ndata: hello\n\n\n\ndata: world\n\n";
        let events = parse_sse_events(raw);
        assert_eq!(events, vec!["hello", "world"]);
    }
}

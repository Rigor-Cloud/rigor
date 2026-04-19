use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

/// Raw entry from a Claude Code transcript JSONL file.
#[derive(Debug, Clone, Deserialize)]
pub struct TranscriptEntry {
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub content: Option<serde_json::Value>,
    #[serde(flatten)]
    pub extra: serde_json::Value,
}

/// Processed message from transcript with extracted text.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptMessage {
    pub role: String,
    pub text: String,
    pub message_index: usize,
}

/// Parse a JSONL transcript file into structured messages.
///
/// Claude Code transcripts use JSONL format (one JSON object per line).
/// Each entry may have:
/// - role: "user" | "assistant" | other
/// - content: string OR array of content blocks
///
/// This parser handles both formats and extracts plain text.
pub fn parse_transcript(path: &Path) -> Result<Vec<TranscriptMessage>> {
    let file = File::open(path)
        .with_context(|| format!("Failed to open transcript file: {}", path.display()))?;
    let reader = BufReader::new(file);

    let mut messages = Vec::new();
    let mut message_index = 0;

    for (line_num, line) in reader.lines().enumerate() {
        let line = line.with_context(|| format!("Failed to read line {}", line_num + 1))?;

        // Skip empty lines
        if line.trim().is_empty() {
            continue;
        }

        // Parse as JSON
        let entry: TranscriptEntry = serde_json::from_str(&line)
            .with_context(|| format!("Failed to parse JSON on line {}", line_num + 1))?;

        // Extract role and content
        let role = match entry.role {
            Some(r) if !r.is_empty() => r,
            _ => continue, // Skip entries without role
        };

        let text = match entry.content {
            Some(serde_json::Value::String(s)) => s,
            Some(serde_json::Value::Array(blocks)) => {
                // Extract text from content blocks
                extract_text_from_blocks(&blocks)
            }
            _ => continue, // Skip entries without valid content
        };

        if !text.is_empty() {
            messages.push(TranscriptMessage {
                role,
                text,
                message_index,
            });
            message_index += 1;
        }
    }

    Ok(messages)
}

/// Extract plain text from an array of content blocks.
///
/// Claude transcript content blocks have format:
/// [{"type": "text", "text": "..."}, ...]
fn extract_text_from_blocks(blocks: &[serde_json::Value]) -> String {
    let mut parts = Vec::new();

    for block in blocks {
        if let Some(obj) = block.as_object() {
            // Check if type == "text"
            if let Some(serde_json::Value::String(block_type)) = obj.get("type") {
                if block_type == "text" {
                    if let Some(serde_json::Value::String(text)) = obj.get("text") {
                        parts.push(text.clone());
                    }
                }
            }
        }
    }

    parts.join("\n")
}

/// Get the latest assistant message from a list of messages.
pub fn get_latest_assistant_message(messages: &[TranscriptMessage]) -> Option<&TranscriptMessage> {
    messages.iter().rev().find(|msg| msg.role == "assistant")
}

/// Get all assistant messages from a list of messages.
pub fn get_assistant_messages(messages: &[TranscriptMessage]) -> Vec<&TranscriptMessage> {
    messages
        .iter()
        .filter(|msg| msg.role == "assistant")
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_parse_transcript_with_string_content() {
        let mut tmpfile = NamedTempFile::new().unwrap();
        writeln!(
            tmpfile,
            r#"{{"role":"user","content":"Hello, how are you?"}}"#
        )
        .unwrap();
        writeln!(
            tmpfile,
            r#"{{"role":"assistant","content":"I'm doing well, thank you!"}}"#
        )
        .unwrap();
        tmpfile.flush().unwrap();

        let messages = parse_transcript(tmpfile.path()).unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].text, "Hello, how are you?");
        assert_eq!(messages[0].message_index, 0);
        assert_eq!(messages[1].role, "assistant");
        assert_eq!(messages[1].text, "I'm doing well, thank you!");
        assert_eq!(messages[1].message_index, 1);
    }

    #[test]
    fn test_parse_transcript_with_array_content() {
        let mut tmpfile = NamedTempFile::new().unwrap();
        writeln!(
            tmpfile,
            r#"{{"role":"user","content":[{{"type":"text","text":"What is Rust?"}}]}}"#
        )
        .unwrap();
        writeln!(
            tmpfile,
            r#"{{"role":"assistant","content":[{{"type":"text","text":"Rust is a systems programming language."}},{{"type":"text","text":"It emphasizes safety and performance."}}]}}"#
        )
        .unwrap();
        tmpfile.flush().unwrap();

        let messages = parse_transcript(tmpfile.path()).unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].text, "What is Rust?");
        assert_eq!(messages[1].role, "assistant");
        assert_eq!(
            messages[1].text,
            "Rust is a systems programming language.\nIt emphasizes safety and performance."
        );
    }

    #[test]
    fn test_get_latest_assistant_message() {
        let messages = vec![
            TranscriptMessage {
                role: "user".to_string(),
                text: "Hello".to_string(),
                message_index: 0,
            },
            TranscriptMessage {
                role: "assistant".to_string(),
                text: "Hi there!".to_string(),
                message_index: 1,
            },
            TranscriptMessage {
                role: "user".to_string(),
                text: "How are you?".to_string(),
                message_index: 2,
            },
            TranscriptMessage {
                role: "assistant".to_string(),
                text: "I'm doing great!".to_string(),
                message_index: 3,
            },
        ];

        let latest = get_latest_assistant_message(&messages).unwrap();
        assert_eq!(latest.text, "I'm doing great!");
        assert_eq!(latest.message_index, 3);
    }

    #[test]
    fn test_get_assistant_messages() {
        let messages = vec![
            TranscriptMessage {
                role: "user".to_string(),
                text: "Hello".to_string(),
                message_index: 0,
            },
            TranscriptMessage {
                role: "assistant".to_string(),
                text: "Hi there!".to_string(),
                message_index: 1,
            },
            TranscriptMessage {
                role: "user".to_string(),
                text: "How are you?".to_string(),
                message_index: 2,
            },
            TranscriptMessage {
                role: "assistant".to_string(),
                text: "I'm doing great!".to_string(),
                message_index: 3,
            },
        ];

        let assistant_msgs = get_assistant_messages(&messages);
        assert_eq!(assistant_msgs.len(), 2);
        assert_eq!(assistant_msgs[0].text, "Hi there!");
        assert_eq!(assistant_msgs[1].text, "I'm doing great!");
    }

    #[test]
    fn test_parse_transcript_skips_empty_lines() {
        let mut tmpfile = NamedTempFile::new().unwrap();
        writeln!(tmpfile, r#"{{"role":"user","content":"First"}}"#).unwrap();
        writeln!(tmpfile).unwrap();
        writeln!(tmpfile, r#"{{"role":"assistant","content":"Second"}}"#).unwrap();
        tmpfile.flush().unwrap();

        let messages = parse_transcript(tmpfile.path()).unwrap();
        assert_eq!(messages.len(), 2);
    }

    #[test]
    fn test_parse_transcript_skips_entries_without_role() {
        let mut tmpfile = NamedTempFile::new().unwrap();
        writeln!(tmpfile, r#"{{"content":"No role here"}}"#).unwrap();
        writeln!(tmpfile, r#"{{"role":"user","content":"Has role"}}"#).unwrap();
        tmpfile.flush().unwrap();

        let messages = parse_transcript(tmpfile.path()).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].text, "Has role");
    }
}

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Result;

/// Rigor global config file: ~/.rigor/config
/// Simple key=value format, one per line. Lines starting with # are comments.
///
/// Supported keys:
///   judge.api_key   — API key for LLM-as-judge calls (e.g. OpenRouter key)
///   judge.api_url   — Base URL for judge API (default: https://openrouter.ai/api)
///   judge.model     — Model for judge calls (default: anthropic/claude-sonnet-4-6)
fn config_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".rigor").join("config")
}

fn load_config() -> HashMap<String, String> {
    let path = config_path();
    let mut map = HashMap::new();
    if let Ok(content) = std::fs::read_to_string(&path) {
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = trimmed.split_once('=') {
                map.insert(key.trim().to_string(), value.trim().to_string());
            }
        }
    }
    map
}

fn save_config(map: &HashMap<String, String>) -> Result<()> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut lines = Vec::new();
    lines.push("# Rigor global configuration".to_string());
    lines.push("# Set with: rigor config set <key> <value>".to_string());
    lines.push(String::new());

    let mut keys: Vec<&String> = map.keys().collect();
    keys.sort();
    for key in keys {
        lines.push(format!("{} = {}", key, map[key]));
    }
    lines.push(String::new());
    std::fs::write(&path, lines.join("\n"))?;
    Ok(())
}

/// Get a config value.
pub fn get(key: &str) -> Option<String> {
    load_config().get(key).cloned()
}

/// Get judge configuration: (api_url, api_key, model)
/// Falls back to env vars, then defaults.
pub fn judge_config() -> (String, Option<String>, String) {
    let config = load_config();

    let api_key = config
        .get("judge.api_key")
        .cloned()
        .or_else(|| std::env::var("RIGOR_JUDGE_API_KEY").ok());

    let api_url = config
        .get("judge.api_url")
        .cloned()
        .or_else(|| std::env::var("RIGOR_JUDGE_API_URL").ok())
        .unwrap_or_else(|| "https://openrouter.ai/api".to_string());

    let model = config
        .get("judge.model")
        .cloned()
        .or_else(|| std::env::var("RIGOR_JUDGE_MODEL").ok())
        .unwrap_or_else(|| "anthropic/claude-sonnet-4-6".to_string());

    (api_url, api_key, model)
}

/// Run `rigor config` subcommand.
pub fn run_config(action: &str, key: Option<&str>, value: Option<&str>) -> Result<()> {
    match action {
        "set" => {
            let key =
                key.ok_or_else(|| anyhow::anyhow!("Usage: rigor config set <key> <value>"))?;
            let value =
                value.ok_or_else(|| anyhow::anyhow!("Usage: rigor config set <key> <value>"))?;
            let mut config = load_config();
            config.insert(key.to_string(), value.to_string());
            save_config(&config)?;
            eprintln!(
                "rigor: set {} = {}",
                key,
                if key.contains("key") {
                    mask_key(value)
                } else {
                    value.to_string()
                }
            );
            Ok(())
        }
        "get" => {
            let key = key.ok_or_else(|| anyhow::anyhow!("Usage: rigor config get <key>"))?;
            match get(key) {
                Some(val) => {
                    let display = if key.contains("key") {
                        mask_key(&val)
                    } else {
                        val
                    };
                    println!("{}", display);
                }
                None => eprintln!("rigor: key '{}' not set", key),
            }
            Ok(())
        }
        "list" => {
            let config = load_config();
            if config.is_empty() {
                eprintln!("rigor: no configuration set");
                eprintln!("rigor: use 'rigor config set <key> <value>' to configure");
            } else {
                for (key, value) in &config {
                    let display = if key.contains("key") {
                        mask_key(value)
                    } else {
                        value.clone()
                    };
                    println!("{} = {}", key, display);
                }
            }
            Ok(())
        }
        _ => anyhow::bail!("Unknown config action '{}'. Use: set, get, list", action),
    }
}

fn mask_key(key: &str) -> String {
    if key.len() <= 8 {
        "****".to_string()
    } else {
        format!("{}...{}", &key[..4], &key[key.len() - 4..])
    }
}

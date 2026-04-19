use anyhow::Context;
use serde::Deserialize;
use std::io::{self, Read};

#[derive(Deserialize, Debug)]
pub struct StopHookInput {
    pub session_id: String,
    pub transcript_path: String,
    pub cwd: String,
    pub permission_mode: String,
    pub hook_event_name: String,
    pub stop_hook_active: bool,
}

impl StopHookInput {
    /// Read and parse hook input from stdin
    pub fn from_stdin() -> anyhow::Result<Self> {
        let mut buffer = String::new();
        io::stdin().read_to_string(&mut buffer)?;

        // Log raw input in debug mode
        if std::env::var("RIGOR_DEBUG").is_ok() {
            eprintln!("rigor: input JSON: {}", buffer);
        }

        let input = serde_json::from_str(&buffer).context("Failed to parse hook input JSON")?;

        Ok(input)
    }
}

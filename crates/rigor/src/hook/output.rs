use serde::Serialize;

#[derive(Serialize, Debug)]
pub struct Metadata {
    pub version: String,
    pub constraint_count: usize,
    pub claim_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct HookResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub metadata: Metadata,
}

impl HookResponse {
    /// Create an "allow" response (no blocking)
    pub fn allow() -> Self {
        Self {
            decision: None,
            reason: None,
            metadata: Metadata {
                version: env!("CARGO_PKG_VERSION").to_string(),
                constraint_count: 0,
                claim_count: 0,
                error: None,
                error_message: None,
            },
        }
    }

    /// Create a "block" response with reason
    pub fn block(reason: impl Into<String>) -> Self {
        Self {
            decision: Some("block".to_string()),
            reason: Some(reason.into()),
            metadata: Metadata {
                version: env!("CARGO_PKG_VERSION").to_string(),
                constraint_count: 0,
                claim_count: 0,
                error: None,
                error_message: None,
            },
        }
    }

    /// Create an "allow" response with error metadata (fail open)
    pub fn error(error_message: impl Into<String>) -> Self {
        Self {
            decision: None,
            reason: None,
            metadata: Metadata {
                version: env!("CARGO_PKG_VERSION").to_string(),
                constraint_count: 0,
                claim_count: 0,
                error: Some(true),
                error_message: Some(error_message.into()),
            },
        }
    }

    /// Write response to stdout as JSON
    pub fn write_stdout(&self) -> anyhow::Result<()> {
        let json = serde_json::to_string(self)?;
        println!("{}", json);
        Ok(())
    }
}

use serde::{Deserialize, Serialize};

/// SessionMetadata captures the context of a rigor execution session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadata {
    /// Unique session identifier (UUID v4)
    pub session_id: String,
    /// ISO 8601 timestamp when session started
    pub timestamp: String,
    /// Git commit hash (short form, 8 chars) if in a git repo
    pub git_commit: Option<String>,
    /// Whether the working tree has uncommitted changes
    pub git_dirty: bool,
}

impl Default for SessionMetadata {
    fn default() -> Self {
        Self {
            session_id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            git_commit: None,
            git_dirty: false,
        }
    }
}

/// ViolationLogEntry represents a single constraint violation event.
/// These are appended to ~/.rigor/violations.jsonl for analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViolationLogEntry {
    /// Session context
    pub session: SessionMetadata,
    /// Unique constraint identifier
    pub constraint_id: String,
    /// Human-readable constraint name
    pub constraint_name: String,
    /// IDs of claims that triggered this violation
    pub claim_ids: Vec<String>,
    /// Text of claims that triggered this violation
    pub claim_text: Vec<String>,
    /// Base strength of constraint (before argumentation)
    pub base_strength: f64,
    /// Computed strength after argumentation framework
    pub computed_strength: f64,
    /// Severity level: "block" | "warn" | "allow"
    pub severity: String,
    /// Final decision: "block" | "allow"
    pub decision: String,
    /// Human-readable violation message
    pub message: String,
    /// Constraint IDs that support this constraint
    pub supporters: Vec<String>,
    /// Constraint IDs that attack this constraint
    pub attackers: Vec<String>,
    /// Total number of claims evaluated in this session
    pub total_claims: usize,
    /// Total number of constraints evaluated in this session
    pub total_constraints: usize,
    /// Path to the source transcript file
    #[serde(default)]
    pub transcript_path: Option<String>,
    /// Confidence score of the claim that triggered this violation
    #[serde(default)]
    pub claim_confidence: Option<f64>,
    /// Type of claim (assertion, negation, code_reference, etc.)
    #[serde(default)]
    pub claim_type: Option<String>,
    /// Source location within transcript (message index, sentence index)
    #[serde(default)]
    pub claim_source: Option<ClaimSource>,
    /// User annotation: was this a false positive?
    pub false_positive: Option<bool>,
    /// User annotation: free-form notes
    pub annotation_note: Option<String>,
    /// Model identifier (e.g. "claude-opus-4-7") — captured from proxy requests.
    #[serde(default)]
    pub model: Option<String>,
}

/// Source provenance for a claim — traces back to transcript location.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimSource {
    /// Index of the assistant message in the transcript
    pub message_index: usize,
    /// Index of the sentence within that message
    pub sentence_index: usize,
}

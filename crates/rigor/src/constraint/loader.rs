use super::types::RigorConfig;
use super::validator::ConstraintValidator;
use anyhow::{Context, Result};
use std::path::Path;

/// Load and validate a rigor.yaml configuration file.
///
/// Two-stage process: parse YAML first, then validate schema constraints.
pub fn load_rigor_config(path: &Path) -> Result<RigorConfig> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read rigor.yaml at {}", path.display()))?;

    let config: RigorConfig = serde_yml::from_str(&content)
        .with_context(|| format!("Failed to parse rigor.yaml at {}", path.display()))?;

    ConstraintValidator::validate(&config)?;

    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const VALID_YAML: &str = r#"
constraints:
  beliefs:
    - id: b1
      epistemic_type: belief
      name: "No hallucinated APIs"
      description: "APIs must exist"
      rego: "package rigor.b1\nviolation[msg] { false }"
      message: "Hallucinated API detected"
      tags: ["api"]
    - id: b2
      epistemic_type: belief
      name: "No phantom deps"
      description: "Deps must be real"
      rego: "package rigor.b2\nviolation[msg] { false }"
      message: "Phantom dependency"
  justifications:
    - id: j1
      epistemic_type: justification
      name: "Source citation"
      description: "Claims need sources"
      rego: "package rigor.j1\nviolation[msg] { false }"
      message: "Missing citation"
  defeaters:
    - id: d1
      epistemic_type: defeater
      name: "Deprecated check"
      description: "Detect deprecated usage"
      rego: "package rigor.d1\nviolation[msg] { false }"
      message: "Using deprecated API"
relations:
  - from: j1
    to: b1
    relation_type: supports
  - from: d1
    to: b2
    relation_type: attacks
"#;

    #[test]
    fn test_load_valid_yaml() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        write!(tmp, "{}", VALID_YAML).unwrap();
        let config = load_rigor_config(tmp.path()).unwrap();
        assert_eq!(config.constraints.beliefs.len(), 2);
        assert_eq!(config.constraints.justifications.len(), 1);
        assert_eq!(config.constraints.defeaters.len(), 1);
        assert_eq!(config.relations.len(), 2);
        assert_eq!(config.all_constraints().len(), 4);
    }

    #[test]
    fn test_load_invalid_yaml() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        write!(tmp, "{{{{not valid yaml at all").unwrap();
        let result = load_rigor_config(tmp.path());
        assert!(result.is_err());
        let err = format!("{:#}", result.unwrap_err());
        assert!(err.contains("Failed to parse"), "got: {}", err);
    }

    #[test]
    fn test_load_missing_file() {
        let result = load_rigor_config(Path::new("/nonexistent/rigor.yaml"));
        assert!(result.is_err());
        let err = format!("{:#}", result.unwrap_err());
        assert!(err.contains("Failed to read"), "got: {}", err);
    }
}

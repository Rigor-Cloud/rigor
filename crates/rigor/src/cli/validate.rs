use std::path::PathBuf;
use std::process;

use anyhow::Result;

use crate::constraint::loader::load_rigor_config;

use super::find_rigor_yaml;

pub fn run_validate(path: Option<PathBuf>) -> Result<()> {
    let yaml_path = find_rigor_yaml(path)?;

    match load_rigor_config(&yaml_path) {
        Ok(config) => {
            let constraint_count = config.all_constraints().len();
            let relation_count = config.relations.len();
            println!(
                "\u{2713} rigor.yaml is valid ({} constraints, {} relations)",
                constraint_count, relation_count
            );
            Ok(())
        }
        Err(e) => {
            eprintln!("\u{2717} rigor.yaml has errors:");
            eprintln!("  {:#}", e);
            process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_valid_config() {
        let yaml = r#"
constraints:
  beliefs:
    - id: b1
      epistemic_type: belief
      name: "Test"
      description: "A test"
      rego: "package rigor.b1\nviolation[msg] { false }"
      message: "test"
  justifications: []
  defeaters: []
relations: []
"#;
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), yaml).unwrap();
        let result = run_validate(Some(tmp.path().to_path_buf()));
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_missing_file() {
        let result = run_validate(Some(PathBuf::from("/nonexistent/rigor.yaml")));
        assert!(result.is_err());
    }
}

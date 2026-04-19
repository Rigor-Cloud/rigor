use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;

use anyhow::Result;

use crate::constraint::graph::ArgumentationGraph;
use crate::constraint::loader::load_rigor_config;
use crate::constraint::types::RelationType;

use super::find_rigor_yaml;

pub fn run_show(path: Option<PathBuf>) -> Result<()> {
    let yaml_path = find_rigor_yaml(path)?;
    let config = load_rigor_config(&yaml_path)?;

    let mut graph = ArgumentationGraph::from_config(&config);
    graph.compute_strengths()?;

    let use_color = io::stdout().is_terminal();
    let mut out = io::stdout().lock();

    let total = config.all_constraints().len();
    writeln!(
        out,
        "Constraints ({} loaded from {})",
        total,
        yaml_path.display()
    )?;
    writeln!(out)?;

    let beliefs = &config.constraints.beliefs;
    let justifications = &config.constraints.justifications;
    let defeaters = &config.constraints.defeaters;

    for (label, constraints) in [
        ("BELIEFS", beliefs.as_slice()),
        ("JUSTIFICATIONS", justifications.as_slice()),
        ("DEFEATERS", defeaters.as_slice()),
    ] {
        if constraints.is_empty() {
            continue;
        }
        writeln!(out, "  {}", label)?;
        for c in constraints {
            let strength = graph.get_strength(&c.id).unwrap_or(0.0);
            let (tag, color_start, color_end) = severity_tag(strength, use_color);
            writeln!(
                out,
                "    {} {:<30} strength: {:.2}  {}{}{}",
                bullet(),
                c.name,
                strength,
                color_start,
                tag,
                color_end
            )?;
        }
        writeln!(out)?;
    }

    // Relations
    if !config.relations.is_empty() {
        writeln!(out, "  RELATIONS")?;
        for r in &config.relations {
            let arrow = match r.relation_type {
                RelationType::Supports => "supports",
                RelationType::Attacks => "attacks",
                RelationType::Undercuts => "undercuts",
            };
            writeln!(
                out,
                "    {} \u{2500}\u{2500}{}\u{2500}\u{2500}\u{25B6} {}",
                r.from, arrow, r.to
            )?;
        }
        writeln!(out)?;
    }

    writeln!(
        out,
        "Thresholds: block \u{2265}0.70 \u{2502} warn \u{2265}0.40 \u{2502} allow <0.40"
    )?;

    Ok(())
}

fn bullet() -> &'static str {
    "\u{25CF}"
}

fn severity_tag(strength: f64, use_color: bool) -> (&'static str, &'static str, &'static str) {
    if strength >= 0.70 {
        if use_color {
            ("[BLOCK]", "\x1b[31m", "\x1b[0m")
        } else {
            ("[BLOCK]", "", "")
        }
    } else if strength >= 0.40 {
        if use_color {
            ("[WARN]", "\x1b[33m", "\x1b[0m")
        } else {
            ("[WARN]", "", "")
        }
    } else if use_color {
        ("[ALLOW]", "\x1b[32m", "\x1b[0m")
    } else {
        ("[ALLOW]", "", "")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_severity_tags() {
        assert_eq!(severity_tag(0.80, false).0, "[BLOCK]");
        assert_eq!(severity_tag(0.70, false).0, "[BLOCK]");
        assert_eq!(severity_tag(0.50, false).0, "[WARN]");
        assert_eq!(severity_tag(0.40, false).0, "[WARN]");
        assert_eq!(severity_tag(0.39, false).0, "[ALLOW]");
        assert_eq!(severity_tag(0.10, false).0, "[ALLOW]");
    }

    #[test]
    fn test_show_output_contains_constraints() {
        let yaml = create_test_yaml();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), yaml).unwrap();

        // We can't easily capture stdout, but we can verify it doesn't error
        let result = run_show(Some(tmp.path().to_path_buf()));
        assert!(result.is_ok());
    }

    fn create_test_yaml() -> &'static str {
        r#"
constraints:
  beliefs:
    - id: b1
      epistemic_type: belief
      name: "Test belief"
      description: "A test"
      rego: "package rigor.b1\nviolation[msg] { false }"
      message: "test"
  justifications: []
  defeaters: []
relations: []
"#
    }
}

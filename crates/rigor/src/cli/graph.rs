use std::io::{self, Write};
use std::path::PathBuf;

use anyhow::Result;

use crate::constraint::graph::ArgumentationGraph;
use crate::constraint::loader::load_rigor_config;
use crate::constraint::types::{EpistemicType, RelationType};

use super::find_rigor_yaml;

pub fn run_graph(path: Option<PathBuf>) -> Result<()> {
    let yaml_path = find_rigor_yaml(path)?;
    let config = load_rigor_config(&yaml_path)?;

    let mut graph = ArgumentationGraph::from_config(&config);
    graph.compute_strengths()?;

    let mut out = io::stdout().lock();

    writeln!(out, "digraph rigor {{")?;
    writeln!(out, "  rankdir=LR;")?;
    writeln!(out, "  node [shape=box, style=filled];")?;
    writeln!(out)?;

    // Nodes
    for c in config.all_constraints() {
        let strength = graph.get_strength(&c.id).unwrap_or(0.0);
        let fill = match c.epistemic_type {
            EpistemicType::Belief => "#ffcccc",
            EpistemicType::Justification => "#ccffcc",
            EpistemicType::Defeater => "#ccccff",
        };
        let type_label = match c.epistemic_type {
            EpistemicType::Belief => "belief",
            EpistemicType::Justification => "justification",
            EpistemicType::Defeater => "defeater",
        };
        writeln!(
            out,
            "  \"{}\" [label=\"{}\\n[{}] {:.2}\", fillcolor=\"{}\"];",
            c.id, c.id, type_label, strength, fill
        )?;
    }

    writeln!(out)?;

    // Edges
    for r in &config.relations {
        let (label, color, style) = match r.relation_type {
            RelationType::Supports => ("supports", "green", "solid"),
            RelationType::Attacks => ("attacks", "red", "dashed"),
            RelationType::Undercuts => ("undercuts", "orange", "dotted"),
        };
        writeln!(
            out,
            "  \"{}\" -> \"{}\" [label=\"{}\", color={}, style={}];",
            r.from, r.to, label, color, style
        )?;
    }

    writeln!(out, "}}")?;

    // Hint to stderr
    eprintln!("# Pipe to: dot -Tpng -o rigor-graph.png");

    Ok(())
}

/// Generate DOT output as a string (for testing).
pub fn generate_dot(path: &std::path::Path) -> Result<String> {
    let config = load_rigor_config(path)?;
    let mut graph = ArgumentationGraph::from_config(&config);
    graph.compute_strengths()?;

    let mut out = String::new();
    use std::fmt::Write as FmtWrite;

    writeln!(out, "digraph rigor {{")?;
    writeln!(out, "  rankdir=LR;")?;
    writeln!(out, "  node [shape=box, style=filled];")?;
    writeln!(out)?;

    for c in config.all_constraints() {
        let strength = graph.get_strength(&c.id).unwrap_or(0.0);
        let fill = match c.epistemic_type {
            EpistemicType::Belief => "#ffcccc",
            EpistemicType::Justification => "#ccffcc",
            EpistemicType::Defeater => "#ccccff",
        };
        let type_label = match c.epistemic_type {
            EpistemicType::Belief => "belief",
            EpistemicType::Justification => "justification",
            EpistemicType::Defeater => "defeater",
        };
        writeln!(
            out,
            "  \"{}\" [label=\"{}\\n[{}] {:.2}\", fillcolor=\"{}\"];",
            c.id, c.id, type_label, strength, fill
        )?;
    }

    writeln!(out)?;

    for r in &config.relations {
        let (label, color, style) = match r.relation_type {
            RelationType::Supports => ("supports", "green", "solid"),
            RelationType::Attacks => ("attacks", "red", "dashed"),
            RelationType::Undercuts => ("undercuts", "orange", "dotted"),
        };
        writeln!(
            out,
            "  \"{}\" -> \"{}\" [label=\"{}\", color={}, style={}];",
            r.from, r.to, label, color, style
        )?;
    }

    writeln!(out, "}}")?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_yaml() -> &'static str {
        r#"
constraints:
  beliefs:
    - id: b1
      epistemic_type: belief
      name: "Test belief"
      description: "A test"
      rego: "package rigor.b1\nviolation[msg] { false }"
      message: "test"
  justifications:
    - id: j1
      epistemic_type: justification
      name: "Test justification"
      description: "A test"
      rego: "package rigor.j1\nviolation[msg] { false }"
      message: "test"
  defeaters: []
relations:
  - from: j1
    to: b1
    relation_type: supports
"#
    }

    #[test]
    fn test_dot_output_contains_digraph() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), test_yaml()).unwrap();
        let dot = generate_dot(tmp.path()).unwrap();
        assert!(dot.contains("digraph rigor"), "missing digraph declaration");
    }

    #[test]
    fn test_dot_output_contains_nodes() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), test_yaml()).unwrap();
        let dot = generate_dot(tmp.path()).unwrap();
        assert!(dot.contains("\"b1\""), "missing b1 node");
        assert!(dot.contains("\"j1\""), "missing j1 node");
        assert!(dot.contains("[belief]"), "missing belief type label");
        assert!(
            dot.contains("[justification]"),
            "missing justification type label"
        );
    }

    #[test]
    fn test_dot_output_contains_edges() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), test_yaml()).unwrap();
        let dot = generate_dot(tmp.path()).unwrap();
        assert!(dot.contains("\"j1\" -> \"b1\""), "missing j1->b1 edge");
        assert!(dot.contains("supports"), "missing supports label");
    }
}

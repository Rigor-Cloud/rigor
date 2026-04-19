//! Rust language defaults — fundamental truths about the Rust language
//! that an LLM could easily misrepresent.

/// Returns YAML string of Rust language constraints.
pub fn rust_language_constraints() -> &'static str {
    r#"
    # ── Rust Language Defaults ──────────────────────────────────────
    # Truths about the Rust language itself. Auto-included for Rust projects.

    - id: rust-no-gc
      epistemic_type: belief
      name: "Rust Has No Garbage Collector"
      description: "Rust uses ownership and borrowing for memory management, not garbage collection. Claims that Rust has a GC are wrong."
      source:
        - path: Cargo.toml
          anchor: "[package]"
          context: "This is a Rust project — ownership model applies"
      rego: |
        violation contains v if {
          some c in input.claims
          regex.match(`(?i)rust.*(garbage.collect|gc|tracing.gc|mark.sweep|reference.count)`, c.text)
          not regex.match(`(?i)(no gc|no garbage|doesn.t have|lacks|without)`, c.text)
          v := {
            "constraint_id": "rust-no-gc",
            "violated": true,
            "claims": [c.id],
            "reason": "Rust does not have garbage collection — it uses ownership and borrowing"
          }
        }
      message: "Rust does not have a garbage collector"
      tags: ["rust", "language", "memory"]
      domain: "rust"

    - id: rust-no-null
      epistemic_type: belief
      name: "Rust Has No Null"
      description: "Rust has no null/nil/None pointer. It uses Option<T> for optional values. Claims about null in Rust are wrong."
      source:
        - path: Cargo.toml
          anchor: "[package]"
          context: "Rust project — no null, uses Option<T>"
      rego: |
        violation contains v if {
          some c in input.claims
          regex.match(`(?i)rust.*(null pointer|null reference|nil|nullptr|NullPointerException)`, c.text)
          not regex.match(`(?i)(no null|doesn.t have null|Option|None)`, c.text)
          v := {
            "constraint_id": "rust-no-null",
            "violated": true,
            "claims": [c.id],
            "reason": "Rust has no null — it uses Option<T> for optional values"
          }
        }
      message: "Rust has no null — uses Option<T>"
      tags: ["rust", "language", "types"]
      domain: "rust"

    - id: rust-no-exceptions
      epistemic_type: belief
      name: "Rust Has No Exceptions"
      description: "Rust uses Result<T, E> for error handling, not try/catch exceptions. panic! exists but is not exception handling."
      source:
        - path: Cargo.toml
          anchor: "[package]"
          context: "Rust project — Result<T,E> not exceptions"
      rego: |
        violation contains v if {
          some c in input.claims
          regex.match(`(?i)rust.*(try.catch|throw|exception|except|catch.block)`, c.text)
          not regex.match(`(?i)(no exception|doesn.t have|Result|panic)`, c.text)
          v := {
            "constraint_id": "rust-no-exceptions",
            "violated": true,
            "claims": [c.id],
            "reason": "Rust uses Result<T,E> for error handling, not try/catch exceptions"
          }
        }
      message: "Rust has no exceptions — uses Result<T,E>"
      tags: ["rust", "language", "errors"]
      domain: "rust"

    - id: rust-no-inheritance
      epistemic_type: belief
      name: "Rust Has No Class Inheritance"
      description: "Rust uses traits and composition, not class inheritance. There are no classes in Rust."
      source:
        - path: Cargo.toml
          anchor: "[package]"
          context: "Rust project — traits not classes"
      rego: |
        violation contains v if {
          some c in input.claims
          regex.match(`(?i)rust.*(class|inherit|extends|superclass|subclass|override method)`, c.text)
          not regex.match(`(?i)(no class|no inherit|trait|doesn.t have|composition)`, c.text)
          v := {
            "constraint_id": "rust-no-inheritance",
            "violated": true,
            "claims": [c.id],
            "reason": "Rust has no classes or inheritance — it uses traits and composition"
          }
        }
      message: "Rust has no class inheritance — uses traits"
      tags: ["rust", "language", "oop"]
      domain: "rust"

    - id: rust-ownership
      epistemic_type: belief
      name: "Rust Ownership Rules"
      description: "Each value has exactly one owner. Borrowing: either one &mut OR many &, never both simultaneously."
      source:
        - path: Cargo.toml
          anchor: "[package]"
          context: "Rust project — ownership rules apply"
      rego: |
        violation contains v if {
          some c in input.claims
          regex.match(`(?i)rust.*(multiple owners|shared ownership|no ownership|copy by default)`, c.text)
          not regex.match(`(?i)(Rc|Arc|Clone|shared_ptr)`, c.text)
          v := {
            "constraint_id": "rust-ownership",
            "violated": true,
            "claims": [c.id],
            "reason": "Each Rust value has exactly one owner — shared ownership requires Rc/Arc"
          }
        }
      message: "Incorrect Rust ownership claim"
      tags: ["rust", "language", "ownership"]
      domain: "rust"
"#
}

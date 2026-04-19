//! Go language defaults — fundamental truths about Go
//! that an LLM could easily misrepresent.

pub fn go_language_constraints() -> &'static str {
    r#"
    # ── Go Language Defaults ────────────────────────────────────────
    # Truths about the Go language itself. Auto-included for Go projects.

    - id: go-no-generics-before-1-18
      epistemic_type: belief
      name: "Go Generics Require 1.18+"
      description: "Go generics (type parameters) were added in Go 1.18. Claims about generics in earlier versions are wrong."
      source:
        - path: go.mod
          anchor: "go "
          context: "Go version declaration"
      rego: |
        violation contains v if {
          some c in input.claims
          regex.match(`(?i)go.*(1\.1[0-7]|1\.[0-9][^0-9]).*(generic|type param)`, c.text)
          v := {
            "constraint_id": "go-no-generics-before-1-18",
            "violated": true,
            "claims": [c.id],
            "reason": "Go generics require Go 1.18+, not available in the version claimed"
          }
        }
      message: "Go generics require 1.18+"
      tags: ["go", "language", "generics"]
      domain: "go"

    - id: go-goroutines-not-threads
      epistemic_type: belief
      name: "Goroutines Are Not OS Threads"
      description: "Goroutines are multiplexed onto OS threads by the Go runtime scheduler. They are NOT 1:1 OS threads."
      source:
        - path: go.mod
          anchor: "module"
          context: "Go project — goroutine model applies"
      rego: |
        violation contains v if {
          some c in input.claims
          regex.match(`(?i)goroutine.*(is a|are) (os |operating system |kernel |native )thread`, c.text)
          v := {
            "constraint_id": "go-goroutines-not-threads",
            "violated": true,
            "claims": [c.id],
            "reason": "Goroutines are NOT OS threads — they are multiplexed by the Go runtime (M:N scheduling)"
          }
        }
      message: "Goroutines are not OS threads"
      tags: ["go", "language", "concurrency"]
      domain: "go"

    - id: go-no-classes
      epistemic_type: belief
      name: "Go Has No Classes"
      description: "Go uses structs with methods and interfaces, not classes with inheritance. There is no 'extends' or 'implements' keyword."
      source:
        - path: go.mod
          anchor: "module"
          context: "Go project — no OOP classes"
      rego: |
        violation contains v if {
          some c in input.claims
          regex.match(`(?i)go.*(class |inherit|extends|superclass|subclass|override method)`, c.text)
          not regex.match(`(?i)(no class|no inherit|interface|struct|doesn.t have|embedding)`, c.text)
          v := {
            "constraint_id": "go-no-classes",
            "violated": true,
            "claims": [c.id],
            "reason": "Go has no classes or inheritance — it uses structs, methods, and interfaces"
          }
        }
      message: "Go has no classes — uses structs and interfaces"
      tags: ["go", "language", "oop"]
      domain: "go"

    - id: go-error-handling
      epistemic_type: belief
      name: "Go Uses Explicit Error Returns"
      description: "Go uses explicit error returns (value, error), not try/catch exceptions. panic/recover exists but is not idiomatic error handling."
      source:
        - path: go.mod
          anchor: "module"
          context: "Go project — explicit error handling"
      rego: |
        violation contains v if {
          some c in input.claims
          regex.match(`(?i)go.*(try.catch|exception|throw|except|catch.block)`, c.text)
          not regex.match(`(?i)(no exception|doesn.t have|error return|panic|recover)`, c.text)
          v := {
            "constraint_id": "go-error-handling",
            "violated": true,
            "claims": [c.id],
            "reason": "Go uses explicit (value, error) returns, not try/catch exceptions"
          }
        }
      message: "Go has no exceptions — uses (value, error) returns"
      tags: ["go", "language", "errors"]
      domain: "go"

    - id: go-no-ternary
      epistemic_type: belief
      name: "Go Has No Ternary Operator"
      description: "Go does not have a ternary operator (?:). Use if/else instead."
      source:
        - path: go.mod
          anchor: "module"
          context: "Go project — no ternary"
      rego: |
        violation contains v if {
          some c in input.claims
          regex.match(`(?i)go.*(ternary|conditional operator|\? :|\?\:)`, c.text)
          not regex.match(`(?i)(no ternary|doesn.t have|not supported)`, c.text)
          v := {
            "constraint_id": "go-no-ternary",
            "violated": true,
            "claims": [c.id],
            "reason": "Go does not have a ternary operator — use if/else"
          }
        }
      message: "Go has no ternary operator"
      tags: ["go", "language", "syntax"]
      domain: "go"
"#
}

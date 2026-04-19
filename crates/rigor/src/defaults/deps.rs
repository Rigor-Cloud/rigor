//! Dependency constraint registry — truths about specific crates/packages.
//!
//! Each entry maps a crate name to YAML constraint snippets.
//! Only crates with non-obvious truths (things LLMs get wrong) are included.

/// Look up constraints for a specific package (crate, go module, npm package).
/// Returns None if no constraints are registered for this package.
pub fn constraints_for_crate(name: &str) -> Option<&'static str> {
    // Rust crates
    match name {
        "regorus" => return Some(REGORUS),
        "axum" => return Some(AXUM),
        "tokio" => return Some(TOKIO),
        "reqwest" => return Some(REQWEST),
        _ => {}
    }
    // Go modules (strip full path, match on last segment)
    let go_name = name.rsplit('/').next().unwrap_or(name);
    match go_name {
        "gin" => Some(GIN),
        "echo" => Some(ECHO),
        _ => None,
    }
}

/// List all package names that have registered constraints.
pub fn registered_crates() -> &'static [&'static str] {
    &["regorus", "axum", "tokio", "reqwest", "gin", "echo"]
}

const REGORUS: &str = r#"
    - id: regorus-capabilities
      epistemic_type: belief
      name: "Regorus Is a Subset of OPA"
      description: "regorus is a Rust implementation of a SUBSET of OPA/Rego. It does not support http.send, opa.runtime, net.cidr, or other network/runtime built-ins."
      source:
        - path: Cargo.toml
          anchor: "regorus"
          context: "regorus dependency declaration"
      rego: |
        violation contains v if {
          some c in input.claims
          regex.match(`(?i)regorus.*(supports|provides|enables|can|capable|has)`, c.text)
          regex.match(`(?i)(streaming|async|parallel|hot.reload|watch.mode|interactive|IDE|http\.send|opa\.runtime|net\.cidr)`, c.text)
          v := {
            "constraint_id": "regorus-capabilities",
            "violated": true,
            "claims": [c.id],
            "reason": "regorus is a subset of OPA and does not support this feature"
          }
        }
      message: "regorus is a Rego subset, not full OPA"
      tags: ["regorus", "dependency", "rego"]
      domain: "dependency"
      references:
        - "https://github.com/microsoft/regorus"
"#;

const AXUM: &str = r#"
    - id: axum-is-tower-based
      epistemic_type: belief
      name: "Axum Uses Tower Middleware"
      description: "axum is built on tower and hyper, not actix-web. It uses tower middleware and extractors, not actix guards or handlers."
      source:
        - path: Cargo.toml
          anchor: "axum"
          context: "axum dependency"
      rego: |
        violation contains v if {
          some c in input.claims
          regex.match(`(?i)axum.*(actix|rocket|warp|tide)`, c.text)
          regex.match(`(?i)(uses|built.on|based.on|wraps|powered.by)`, c.text)
          v := {
            "constraint_id": "axum-is-tower-based",
            "violated": true,
            "claims": [c.id],
            "reason": "axum is built on tower/hyper, not the framework claimed"
          }
        }
      message: "axum is tower-based, not actix/rocket/warp"
      tags: ["axum", "dependency", "web"]
      domain: "dependency"
"#;

const TOKIO: &str = r#"
    - id: tokio-is-async-runtime
      epistemic_type: belief
      name: "Tokio Is an Async Runtime"
      description: "tokio is an async runtime using cooperative scheduling on a thread pool. It is NOT an OS-level thread library or green threads implementation."
      source:
        - path: Cargo.toml
          anchor: "tokio"
          context: "tokio dependency"
      rego: |
        violation contains v if {
          some c in input.claims
          regex.match(`(?i)tokio.*(green.thread|preemptive|os.thread|fiber|goroutine)`, c.text)
          not regex.match(`(?i)(not|isn.t|doesn.t|unlike)`, c.text)
          v := {
            "constraint_id": "tokio-is-async-runtime",
            "violated": true,
            "claims": [c.id],
            "reason": "tokio uses cooperative async scheduling, not green threads or preemptive threading"
          }
        }
      message: "tokio is async/cooperative, not green threads"
      tags: ["tokio", "dependency", "async"]
      domain: "dependency"
"#;

const REQWEST: &str = r#"
    - id: reqwest-is-http-client
      epistemic_type: belief
      name: "Reqwest Is an HTTP Client"
      description: "reqwest is an HTTP CLIENT library. It is not a server, framework, or proxy. It supports connection pooling via Client reuse."
      source:
        - path: Cargo.toml
          anchor: "reqwest"
          context: "reqwest dependency"
      rego: |
        violation contains v if {
          some c in input.claims
          regex.match(`(?i)reqwest.*(server|framework|listen|bind|route|handler)`, c.text)
          not regex.match(`(?i)(client|request|send|get|post)`, c.text)
          v := {
            "constraint_id": "reqwest-is-http-client",
            "violated": true,
            "claims": [c.id],
            "reason": "reqwest is an HTTP client library, not a server or framework"
          }
        }
      message: "reqwest is an HTTP client, not a server"
      tags: ["reqwest", "dependency", "http"]
      domain: "dependency"
"#;

// ── Go dependencies ──

const GIN: &str = r#"
    - id: gin-is-http-framework
      epistemic_type: belief
      name: "Gin Is an HTTP Framework"
      description: "Gin is an HTTP web framework for Go, not a full application framework. It does not include an ORM, migrations, or dependency injection."
      source:
        - path: go.mod
          anchor: "gin-gonic/gin"
          context: "gin dependency"
      rego: |
        violation contains v if {
          some c in input.claims
          regex.match(`(?i)gin.*(orm|migration|dependency injection|built.in database|active record)`, c.text)
          v := {
            "constraint_id": "gin-is-http-framework",
            "violated": true,
            "claims": [c.id],
            "reason": "Gin is an HTTP framework only — no ORM, migrations, or DI"
          }
        }
      message: "Gin is an HTTP framework, not a full application framework"
      tags: ["gin", "dependency", "go", "web"]
      domain: "dependency"
"#;

const ECHO: &str = r#"
    - id: echo-is-http-framework
      epistemic_type: belief
      name: "Echo Is an HTTP Framework"
      description: "Echo is a high-performance HTTP framework for Go. It is NOT Gin and has a different API (echo.Context vs gin.Context)."
      source:
        - path: go.mod
          anchor: "labstack/echo"
          context: "echo dependency"
      rego: |
        violation contains v if {
          some c in input.claims
          regex.match(`(?i)echo.*(gin|fiber|chi|gorilla)`, c.text)
          regex.match(`(?i)(same as|identical|built on|wrapper|fork of)`, c.text)
          v := {
            "constraint_id": "echo-is-http-framework",
            "violated": true,
            "claims": [c.id],
            "reason": "Echo is a separate framework from Gin/Fiber/Chi — different API"
          }
        }
      message: "Echo is not Gin — different API and architecture"
      tags: ["echo", "dependency", "go", "web"]
      domain: "dependency"
"#;

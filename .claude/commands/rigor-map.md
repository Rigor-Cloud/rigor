---
name: rigor-map
description: Analyze the codebase and generate epistemic constraints for rigor.yaml. Run this to create or update the constraint graph that grounds AI outputs against your project's facts.
---

You are generating epistemic constraints for the rigor framework. Rigor enforces factual accuracy of AI outputs by evaluating claims against a constraint graph defined in `rigor.yaml`.

## Your task

1. **Read the existing `rigor.yaml`** if it exists — understand what constraints are already defined
2. **Analyze the codebase** — read key files to understand the project:
   - README, main entry point, config files, package manifest (Cargo.toml / package.json / etc.)
   - Core types, exported APIs, key behaviors
   - Architecture: pipeline steps, data flow, module structure
   - Configuration: thresholds, defaults, modes, env vars
3. **Identify facts that must never be misrepresented** — these become constraints:
   - API surface: what functions/types exist and what they do
   - Architecture: how many steps in a pipeline, what depends on what
   - Behavior: error handling mode (fail-open vs fail-closed), default values
   - Dependencies: what libraries are used and what they support
   - Performance: latency budgets, scaling limits
4. **Generate constraints** in rigor.yaml format
5. **Present the constraints to the user** for review before writing

## Constraint YAML format

```yaml
constraints:
  beliefs:
    - id: example-constraint
      name: Human Readable Name
      description: What must be true about this project
      rego: |
        violation contains v if {
          some c in input.claims
          contains(c.text, "wrong thing")
          v := {"constraint_id": "example-constraint", "violated": true, "claims": [c.id], "reason": "Why this is wrong"}
        }
      message: "Human-readable violation message"
      tags: [domain, category]
  justifications:
    - id: evidence-constraint
      name: Evidence Required
      description: Supporting evidence requirement
      rego: |
        violation contains v if {
          some c in input.claims
          # ... rego logic
        }
      message: "Violation message"
      tags: [evidence]
  defeaters:
    - id: defeating-constraint
      name: Defeater Name
      description: What would invalidate a belief
      rego: |
        violation contains v if {
          some c in input.claims
          # ... rego logic
        }
      message: "Violation message"
      tags: [defeater]

relations:
  - from: evidence-constraint
    to: example-constraint
    relation_type: supports
  - from: defeating-constraint
    to: example-constraint
    relation_type: attacks
```

## Rules

- Use **Rego v1 syntax**: `violation contains v if` NOT deprecated `violation[msg]`
- Import helpers: Rego snippets are automatically wrapped in a module with `import data.rigor.helpers`
- Each constraint should catch a **specific factual error**, not be overly broad
- Use `contains(c.text, "keyword")` for text matching — case-sensitive by default, use `lower(c.text)` for case-insensitive
- The constraint_id in the violation object MUST match the constraint's `id` field
- Claims have fields: `id`, `text`, `confidence`, `claim_type`
- Aim for 8-15 constraints per project — enough to cover key facts, not so many that context is bloated

## Epistemic types

- **belief**: Core factual claim about the project (strength 0.8). "regorus is a subset of OPA"
- **justification**: Supporting evidence that strengthens beliefs (strength 0.9). "test evidence required for functional claims"
- **defeater**: Contradicting evidence that weakens beliefs (strength 0.7). "prototype markers defeat production-ready claims"

## Process

1. Read the codebase thoroughly
2. Draft constraints
3. Show them to the user with explanations
4. Ask for approval/changes
5. Write to rigor.yaml (merge with existing if present)
6. Run `rigor validate` to verify the config

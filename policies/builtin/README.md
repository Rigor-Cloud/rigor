# Built-in Constitutional Constraints

Constitutional constraints are universal epistemic rules that any project can adopt. They enforce fundamental principles of epistemic rigor regardless of domain.

## Available Constraints

### no-fabricated-apis

**Package:** `rigor.builtin.no_fabricated_apis`

Detects claims about API features or methods that are fabricated. Catches common hallucination patterns where an AI invents methods, capabilities, or features that don't exist.

**What it catches:**
- Fabricated method names (e.g., `api.magicalMethod`)
- Nonexistent capabilities (e.g., "supports self-healing")
- High-certainty feature claims without documentation references

**Use case:** Any project using external libraries or APIs where hallucinated features would cause runtime errors.

### calibrated-confidence

**Package:** `rigor.builtin.calibrated_confidence`

Enforces epistemic humility by detecting claims with high confidence (>0.85) that use definitive language about inherently uncertain topics like architecture, design, scaling, and future behavior.

**What it catches:**
- Overconfident architectural claims ("This architecture definitely scales")
- Absolute system behavior claims ("This will never crash")
- Future predictions stated as facts ("Users will love this")

**Use case:** Design documents, architecture decisions, and planning outputs where overconfidence leads to poor decisions.

### require-justification

**Package:** `rigor.builtin.require_justification`

Requires functional, performance, and security claims to cite evidence. Claims about correctness, speed, or safety must reference tests, benchmarks, or audits.

**What it catches:**
- Functional claims without test evidence ("works correctly")
- Performance claims without benchmarks ("this is fast")
- Security claims without audits ("this is secure")

**Use case:** Any project where unsubstantiated claims about quality could lead to false confidence in untested code.

## How to Use

Currently, reference built-in constraints by copying the .rego files into your project's policies directory:

```bash
cp policies/builtin/no-fabricated-apis.rego my-project/policies/
```

Then reference them in your `rigor.yaml`:

```yaml
constraints:
  beliefs:
    - id: no-fabricated-apis
      epistemic_type: belief
      name: "No Fabricated APIs"
      rego_file: "policies/no-fabricated-apis.rego"
      message: "Fabricated API feature detected"
      tags: ["api", "hallucination"]
      domain: "general"
```

Future versions will support referencing built-in constraints by ID without copying.

## Design Principles

These constraints follow three principles:

1. **Universal applicability**: They apply to any AI-assisted project
2. **Self-contained**: No dependencies on other files or external data
3. **Graduated severity**: Each constraint has multiple violation clauses catching different severity levels

## Contributing

To add a new constitutional constraint:

1. Create a `.rego` file with `package rigor.builtin.{name}`
2. Use `import rego.v1` syntax
3. Include 2-3 `violation contains v if { ... }` clauses
4. Add comments explaining what each clause catches
5. Update this README with the new constraint's documentation

# Beliefs-Focused Example - External Files & Support Relations

This example demonstrates how to organize constraints into external .rego files and how justifications support beliefs through relations.

## When to Use External .rego Files

Use external .rego files when:

- **Constraints are complex**: Multiple violation clauses or helper functions
- **Reusability matters**: Same constraint used across multiple projects
- **Team organization**: Different people maintain different constraint files
- **Version control**: Easier to track changes to individual constraints

In this example, `no-fabricated-apis` is extracted to `policies/no-fabricated-apis.rego`.

## How Beliefs Form the Foundation

Beliefs are the foundational layer of your constraint system. They express:

- What must be true (API features must exist)
- What must not be true (no fabricated capabilities)
- Domain-specific rules (version compatibility)

In this example:
- `no-fabricated-apis`: APIs must not be fabricated
- `api-accuracy`: API claims must match documentation

## How Supporters Strengthen Beliefs

**Justifications** act as supporters that strengthen beliefs. When test evidence exists, it increases confidence in API claims.

The `relations:` section shows:

```yaml
relations:
  - from: test-evidence-supports
    to: no-fabricated-apis
    relation_type: supports
```

This means: "Test evidence strengthens our confidence that APIs aren't fabricated."

## Running This Example

```bash
cd examples/beliefs-focused/
rigor validate rigor.yaml
```

### Expected Behavior

1. Rigor loads `rigor.yaml`
2. Reads `policies/no-fabricated-apis.rego` (external file)
3. Evaluates both inline and external constraints
4. Applies DF-QuAD to compute final severity based on support relations

### Example Violation

If an AI claims: "This API definitely supports self-healing", the external `no-fabricated-apis.rego` would trigger with reason: "Claimed capability does not exist in the library"

## File Structure

```
beliefs-focused/
├── rigor.yaml              # Main config with constraint references
├── policies/
│   └── no-fabricated-apis.rego  # External constraint policy
└── README.md               # This file
```

## Key Concepts

- **External .rego files**: Referenced via `rego_file:` instead of `rego:`
- **Package naming**: Each .rego file has `package rigor.constraints.{name}`
- **Support relations**: Justifications strengthen beliefs via `supports` type
- **DF-QuAD**: Computes final severity by averaging supporter strengths

## Next Steps

1. Try the **defeaters-focused/** example to learn about attack relations
2. Modify `policies/no-fabricated-apis.rego` to add your own violation rules
3. Create additional external .rego files and reference them in rigor.yaml

## Learn More

- Rego modules: https://www.openpolicyagent.org/docs/latest/policy-language/#modules
- Epistemic foundations: `docs/epistemic-foundations.md`
- DF-QuAD semantics: See research references in project docs

# Basic Example - Inline Constraints

This example teaches the fundamentals of Rigor constraints using inline Rego policies.

## What Are Beliefs?

**Beliefs** are foundational constraints that express what must be true about your AI's claims. Think of them as "hard rules" that claims cannot violate.

In this example, `no-fabricated-apis` is a belief that says: "Claims about API features must not fabricate methods or capabilities that don't exist."

## What Are Justifications?

**Justifications** are constraints that require claims to have evidence or support. They don't say what's true, but rather demand that claims cite their sources.

In this example, `require-evidence` is a justification that says: "If you claim something works, you must reference a test or verification."

## How Inline Rego Works

Each constraint has a `rego:` field containing the policy logic. The policy must:

1. Start with `import rego.v1` (modern Rego syntax)
2. Define one or more `violation contains v if { ... }` rules
3. Check `input.claims` (the claims extracted from AI output)
4. Return violations with `constraint_id`, `violated`, `claims`, and `reason`

## Running This Example

```bash
cd examples/basic/
rigor validate rigor.yaml
```

### Expected Output (No Violations)

If you run this on a clean rigor.yaml with no AI claims, you'll see:

```
No violations detected
```

### Example Violation

If an AI claimed: "This API supports magical auto-healing", the `no-fabricated-apis` belief would trigger:

```
BLOCK: No Fabricated APIs
Reason: Claimed capability does not exist in the library
Confidence: 0.90
Claims: [claim-uuid]
```

## Key Concepts

- **Inline Rego**: Policies embedded directly in rigor.yaml (good for small projects)
- **Beliefs**: Foundational "what must be true" constraints
- **Justifications**: "Show your evidence" constraints
- **Violation structure**: constraint_id, violated, claims, reason

## Next Steps

1. Try the **beliefs-focused/** example to learn about external .rego files
2. Read `docs/epistemic-foundations.md` for theoretical background
3. Experiment by adding your own constraint

## Learn More

- Rego syntax: https://www.openpolicyagent.org/docs/latest/policy-language/
- Epistemic foundations: `docs/epistemic-foundations.md`
- Built-in constraints: `policies/builtin/`

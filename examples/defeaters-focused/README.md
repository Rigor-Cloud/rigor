# Defeaters-Focused Example - Attack Relations & DF-QuAD

This example demonstrates how defeaters attack beliefs and how DF-QuAD computes final severity through mean aggregation.

## How Defeaters Work

**Defeaters** are constraints that attack other constraints, reducing their strength. They represent counter-evidence or contradictions.

Think of defeaters as "yes, but..." constraints:
- "Yes, the code is production-ready, BUT there are prototype markers"
- "Yes, it's complete, BUT features are missing"

## How Attacks Reduce Constraint Strength

When a defeater attacks a belief via an `attacks` relation, DF-QuAD reduces the belief's effective strength.

Example from this config:

```yaml
relations:
  - from: prototype-marker
    to: production-ready
    relation_type: attacks
```

This means: "prototype-marker defeater attacks the production-ready belief"

## DF-QuAD Mean Aggregation

DF-QuAD computes final severity using **mean aggregation**:

1. Start with base constraint strength (e.g., 0.8)
2. Collect all attackers and supporters
3. Compute attacker impact: `mean(attacker_strengths)`
4. Compute supporter impact: `mean(supporter_strengths)`
5. Final strength = `base + supporters - attackers`

**Why mean, not sum?**
- Mean prevents overwhelming by quantity
- A single strong defeater matters more than many weak ones
- Aligns with gradual argumentation semantics from research literature

## Concrete Scenario

Let's trace through a real example:

**Claim**: "This code is production-ready" (confidence: 0.90)

**Evaluation**:
1. `production-ready` belief triggers (base strength: 0.8)
2. `prototype-marker` defeater finds "TODO: fix hack" (attacker strength: 0.7)
3. `test-coverage-supports` finds tests mentioned (supporter strength: 0.6)

**DF-QuAD calculation**:
- Base: 0.8
- Attackers: [0.7] → mean = 0.7
- Supporters: [0.6] → mean = 0.6
- Final: 0.8 + 0.6 - 0.7 = **0.7** (block threshold)

Result: **BLOCK** with reason including both attacker and supporter context.

## Running This Example

```bash
cd examples/defeaters-focused/
rigor validate rigor.yaml
```

### Example Output

If AI claims "production-ready" but code has TODOs:

```
BLOCK: Production Ready
Reason: Production claim contradicted by prototype marker: TODO: refactor this
Confidence: 0.70 (reduced by attacks)
Attackers: prototype-marker (0.7)
Supporters: test-coverage-supports (0.6)
Claims: [claim-uuid-1, claim-uuid-2]
```

## Key Concepts

- **Defeaters**: Constraints that attack other constraints
- **Attack relations**: `relation_type: attacks` creates adversarial links
- **DF-QuAD semantics**: Mean aggregation of attacker/supporter strengths
- **Gradual acceptance**: Results in [0,1] range, not binary true/false
- **Severity thresholds**: block ≥ 0.7, warn ≥ 0.4, allow < 0.4

## Advanced Patterns

### Multiple Attackers

If multiple defeaters attack the same belief:
- Each attacker contributes to mean
- Strong defeaters have more impact than weak ones
- Final severity is gradual, not binary

### Attack Chains

Defeaters can attack other defeaters:
```yaml
- from: test-evidence
  to: prototype-marker
  relation_type: attacks
```

"Test evidence attacks the prototype defeater, reducing its impact"

### Undercuts (v0.1)

Rigor v0.1 treats undercuts as attacks. Future versions may distinguish them.

## Next Steps

1. Experiment with different strength values (0.0 - 1.0)
2. Add more complex attack chains
3. Create scenarios with multiple attackers and supporters
4. Study DF-QuAD computation in violation output

## Learn More

- DF-QuAD paper: Cayrol & Lagasquie-Schiex
- Epistemic foundations: `docs/epistemic-foundations.md`
- Bipolar argumentation: See research references in project docs

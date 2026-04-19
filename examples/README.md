# Rigor Examples

This directory contains example configurations demonstrating different epistemic concepts and constraint patterns.

## Which Example is Right for Me?

| Example | Skill Level | Demonstrates | Best For |
|---------|-------------|--------------|----------|
| **basic/** | Beginner | Inline Rego constraints, basic beliefs and justifications | First-time users learning Rigor syntax |
| **beliefs-focused/** | Intermediate | External .rego files, supports relations | Users organizing constraints into reusable modules |
| **defeaters-focused/** | Advanced | Attack relations, DF-QuAD semantics | Users implementing adversarial constraint systems |

## Running an Example

Each example directory contains a `rigor.yaml` configuration file. To run:

```bash
cd examples/basic/
rigor validate rigor.yaml
```

Replace `basic/` with your chosen example directory.

## What You'll Learn

**Basic Example**: Understand what beliefs and justifications are, how to write inline Rego constraints, and how to interpret violation output.

**Beliefs-Focused Example**: Learn when to extract constraints into separate .rego files, how supporters strengthen beliefs, and how to structure a multi-file constraint project.

**Defeaters-Focused Example**: Explore how defeaters attack beliefs, how DF-QuAD computes final severity via mean aggregation, and how to model adversarial scenarios.

## Next Steps

After exploring these examples:

1. Read `docs/epistemic-foundations.md` for theoretical background
2. Check `policies/builtin/` for reusable constitutional constraints
3. Create your own `rigor.yaml` for your project

## Getting Help

- File issues: https://github.com/Rigor-Cloud/rigor/issues
- Read docs: `docs/`
- Check built-in constraints: `policies/builtin/`

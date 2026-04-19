# Configuration Reference

This document describes the complete `rigor.yaml` configuration schema, CLI commands, environment variables, and file locations.

## Table of Contents

1. [rigor.yaml Schema](#rigoryaml-schema)
2. [Constraint Fields](#constraint-fields)
3. [Relation Fields](#relation-fields)
4. [CLI Commands](#cli-commands)
5. [Environment Variables](#environment-variables)
6. [File Locations](#file-locations)

## rigor.yaml Schema

The `rigor.yaml` file defines the epistemic constraints and argumentation relations that Rigor enforces.

**Top-level structure:**

```yaml
constraints:
  beliefs: []
  justifications: []
  defeaters: []

relations: []
```

### Complete Example

See the repository's `rigor.yaml` for a complete working example with 8 constraints (4 beliefs, 2 justifications, 2 defeaters) and 4 relations.

## Constraint Fields

Constraints are defined under three epistemic categories: `beliefs`, `justifications`, and `defeaters`. All three types use the same field schema.

### Required Fields

| Field | Type | Description |
|-------|------|-------------|
| `id` | string | Unique identifier (kebab-case recommended) |
| `epistemic_type` | string | Must be `belief`, `justification`, or `defeater` |
| `name` | string | Human-readable constraint name |
| `description` | string | What the constraint checks for |
| `rego` | string | Rego policy (inline string, see patterns below) |
| `message` | string | User-facing message when violated |

### Optional Fields

| Field | Type | Description | Default |
|-------|------|-------------|---------|
| `tags` | string[] | Categorization tags (e.g., `["api", "hallucination"]`) | `[]` |
| `domain` | string | Constraint domain (e.g., `"general"`, `"rigor"`) | `"general"` |
| `references` | string[] | URLs or citations supporting the constraint | `[]` |

### Field Details

**`id`**: Used in relations and violation output. Must be unique across all constraints.

**`epistemic_type`**: Determines the constraint's role in argumentation:
- `belief`: Core assertion that can be supported or attacked
- `justification`: Provides evidence supporting other constraints
- `defeater`: Challenges or weakens other constraints

**`rego`**: Inline Rego policy string. Must define a `violation` set using the pattern:

```rego
violation contains v if {
  # condition checks
  v := {
    "constraint_id": "your-constraint-id",
    "violated": true,
    "claims": [claim_ids],
    "reason": "explanation"
  }
}
```

Multiple `violation contains v if` clauses can be defined in a single constraint.

**`message`**: Shown to user when constraint is violated. Keep concise (one sentence).

**`tags`**: Used for filtering and organization. Common tags: `api`, `testing`, `syntax`, `hallucination`, `domain`.

**`domain`**: Organizes constraints by scope. `"general"` applies broadly, domain-specific values (e.g., `"rigor"`, `"regorus"`) apply to project-specific concerns.

**`references`**: Document sources, APIs, or papers that justify the constraint. URLs or citations.

## Relation Fields

Relations define the argumentation graph structure. They connect constraints using `supports` or `attacks` relations.

### Required Fields

| Field | Type | Description |
|-------|------|-------------|
| `from` | string | Source constraint ID |
| `to` | string | Target constraint ID |
| `relation_type` | string | Must be `supports` or `attacks` |

### Relation Types

**`supports`**: The source constraint provides evidence for the target constraint. Increases target's strength.

Example:
```yaml
- from: test-evidence-supports
  to: no-fabricated-apis
  relation_type: supports
```

Meaning: When `test-evidence-supports` is satisfied, it increases confidence in `no-fabricated-apis`.

**`attacks`**: The source constraint challenges the target constraint. Decreases target's strength.

Example:
```yaml
- from: prototype-defeats-strict
  to: no-fabricated-apis
  relation_type: attacks
```

Meaning: When `prototype-defeats-strict` is violated, it weakens confidence in `no-fabricated-apis`.

### DF-QuAD Strength Computation

Rigor uses DF-QuAD gradual semantics to compute final constraint strength:

1. Start with base strength (default: 0.8)
2. Compute mean strength of supporting constraints
3. Compute mean strength of attacking constraints
4. Final strength = base + supporters - attackers (clamped to [0,1])

See [Epistemic Foundations](./epistemic-foundations.md) for details.

## CLI Commands

### No Arguments (Hook Mode)

```bash
rigor
```

Reads stdin (Claude transcript JSON), evaluates constraints, writes decision JSON to stdout. This is the mode used by Claude Code's Stop hook.

### `validate`

```bash
rigor validate [path/to/rigor.yaml]
```

Validates `rigor.yaml` syntax and constraint definitions. Checks:
- YAML parsing
- Required fields present
- Rego syntax valid
- Constraint IDs unique
- Relations reference existing constraints

If no path provided, searches current directory tree for `rigor.yaml`.

### `show`

```bash
rigor show [-p path/to/rigor.yaml]
```

Displays all constraints with computed strengths and severity zones:

```
no-fabricated-apis (belief)
  Strength: 0.80 | Severity: block
  Supporters: 1 | Attackers: 1
  Description: Claims about APIs must not fabricate features
```

### `show hook`

```bash
rigor show hook
```

Outputs the `.claude/settings.local.json` configuration snippet for Claude Code:

```json
{
  "hooks": {
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "/absolute/path/to/rigor"
          }
        ]
      }
    ]
  }
}
```

### `graph`

```bash
rigor graph [-p path/to/rigor.yaml]
```

Outputs constraint argumentation graph in DOT format (Graphviz):

```bash
rigor graph | dot -Tpng -o constraints.png
```

Nodes represent constraints, edges represent relations (green = supports, red = attacks).

### `log` Subcommands

#### `log last`

```bash
rigor log last [N]
```

Display the last N violations (default 10) from `~/.rigor/violations.jsonl`.

Output format:
```
[1] 2026-01-29 12:34:56 | no-fabricated-apis | session abc123de | BLOCK
[2] 2026-01-29 14:22:10 | rego-syntax-accuracy | session def456gh | WARN
```

#### `log constraint`

```bash
rigor log constraint <constraint-id>
```

Filter violations by constraint ID:

```bash
rigor log constraint no-fabricated-apis
```

#### `log session`

```bash
rigor log session <session-id>
```

Filter violations by Claude session ID:

```bash
rigor log session abc123de
```

#### `log stats`

```bash
rigor log stats
```

Display statistics about violations:

```
Total violations: 42
Blocks: 28 (67%)
Warns: 14 (33%)

Top constraints:
  no-fabricated-apis: 15 violations
  rego-syntax-accuracy: 12 violations
  no-false-test-claims: 8 violations
```

#### `log annotate`

```bash
rigor log annotate <entry-number> <annotation>
```

Add annotation to a log entry (identified by 1-based index from `log last`):

```bash
rigor log annotate 1 "This was expected during testing"
```

Annotations are stored in `~/.rigor/violations.jsonl` and displayed with log entries.

## Environment Variables

### `RIGOR_DEBUG`

**Type:** boolean (any value = true, unset = false)
**Purpose:** Enable debug logging to stderr and `~/.rigor/rigor.log`

Example:
```bash
RIGOR_DEBUG=1 rigor
```

Debug output includes:
- Raw input JSON
- Extracted claims with confidence scores
- Rego evaluation traces
- Strength computation details

### `RIGOR_FAIL_CLOSED`

**Type:** boolean (any value = true, unset = false)
**Default:** `false` (fail-open)
**Purpose:** Block output on errors instead of allowing with metadata

Fail-open (default):
- Configuration errors → allow with reason
- Claim extraction errors → allow with empty claims
- Rego evaluation errors → allow with reason

Fail-closed (`RIGOR_FAIL_CLOSED=true`):
- Any error → block with error details

Example:
```bash
RIGOR_FAIL_CLOSED=true rigor
```

**Recommendation:** Use fail-open during development, fail-closed in CI/CD.

### `RIGOR_TEST_CLAIMS`

**Type:** JSON string
**Purpose:** Override claim extraction with test data

Used for testing constraint logic without running Claude Code:

```bash
RIGOR_TEST_CLAIMS='[{"id":"test-1","text":"regorus.Engine.fabricated() is a method","confidence":0.9}]' rigor
```

Claim schema:
```json
{
  "id": "string",
  "text": "string",
  "confidence": 0.0-1.0,
  "claim_type": "string (optional)",
  "source": {
    "message_index": 0,
    "sentence_index": 0
  }
}
```

## File Locations

### `~/.rigor/violations.jsonl`

Persistent log of all violations. Each line is a JSON object:

```json
{
  "timestamp": "2026-01-29T12:34:56Z",
  "session_id": "abc123de",
  "constraint_id": "no-fabricated-apis",
  "severity": "block",
  "claims": ["claim-uuid-1", "claim-uuid-2"],
  "reason": "Fabricated API method detected",
  "git_commit": "7a8d2cb",
  "git_branch": "main",
  "annotation": null
}
```

### `~/.rigor/rigor.log`

Structured debug log (JSON lines). Contains:
- Timestamps
- Log levels (ERROR, WARN, INFO, DEBUG)
- Span traces (for OpenTelemetry integration)
- Error details

### `.claude/settings.local.json`

Claude Code configuration file. Place in project root to configure Rigor as a Stop hook:

```json
{
  "hooks": {
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "/absolute/path/to/rigor"
          }
        ]
      }
    ]
  }
}
```

## Rego Policy Patterns

### Basic Violation Pattern

```rego
violation contains v if {
  some c in input.claims
  # condition
  v := {
    "constraint_id": "your-id",
    "violated": true,
    "claims": [c.id],
    "reason": "explanation"
  }
}
```

### Multiple Clauses

```rego
violation contains v if {
  # First condition
  some c in input.claims
  regex.match(`pattern1`, c.text)
  v := {...}
}

violation contains v if {
  # Second condition
  some c in input.claims
  regex.match(`pattern2`, c.text)
  v := {...}
}
```

### Cross-Claim Comparison

```rego
violation contains v if {
  some c1 in input.claims
  some c2 in input.claims
  c1.id < c2.id  # Avoid duplicates
  contradicts(c1, c2)
  v := {
    "claims": [c1.id, c2.id],
    ...
  }
}
```

### Confidence Thresholds

```rego
violation contains v if {
  some c in input.claims
  c.confidence > 0.8  # High confidence
  # check condition
  v := {...}
}
```

### Regex Matching

Use `regex.match(pattern, string)` for pattern matching:

```rego
regex.match(`(?i)regorus`, c.text)  # Case-insensitive
regex.match(`\bmethod\b`, c.text)   # Word boundaries
```

### Negation

```rego
violation contains v if {
  some c in input.claims
  regex.match(`production ready`, c.text)
  not regex.match(`prototype|experimental`, c.text)  # Require absence
  v := {...}
}
```

## Validation Rules

Rigor enforces these rules on `rigor.yaml`:

1. **Unique IDs**: All constraint IDs must be unique across beliefs, justifications, and defeaters
2. **Valid Rego**: All `rego` fields must parse successfully
3. **Valid Relations**: `from` and `to` in relations must reference existing constraint IDs
4. **Required Fields**: All required fields must be present and non-empty
5. **Epistemic Type Match**: Constraint's `epistemic_type` must match its category (beliefs/justifications/defeaters)

Run `rigor validate` to check your configuration.

## Best Practices

1. **Start Small**: Begin with 2-3 high-value constraints, expand based on observed violations
2. **Use Relations Sparingly**: Only create relations when there's clear support/attack semantics
3. **Test Constraints**: Use `RIGOR_TEST_CLAIMS` to verify constraint logic before deploying
4. **Version Control rigor.yaml**: Commit constraint changes with code changes they protect
5. **Review Logs**: Periodically run `rigor log stats` to identify noisy constraints
6. **Annotate False Positives**: Use `rigor log annotate` to document why violations were acceptable

## Troubleshooting

**Rigor blocks all output**: Check `~/.rigor/rigor.log` for errors. Likely causes:
- Invalid `rigor.yaml` syntax
- Rego constraint errors
- Constraint too strict (adjust regex or confidence thresholds)

**Rigor never blocks**: Verify:
- Hook is configured correctly (`.claude/settings.local.json`)
- Constraints target the right patterns (test with `RIGOR_TEST_CLAIMS`)
- Claims are being extracted (enable `RIGOR_DEBUG`)

**Claims not extracted**: Ensure Claude output contains assertive sentences. Heuristic extractor looks for:
- Definitive markers: "is", "are", "will", "does"
- Negation markers: "not", "never", "cannot"
- Confidence markers: "might", "could", "likely"

**Rego errors**: Use `rigor validate` to check syntax. Common issues:
- Missing `violation contains v` pattern
- Incorrect `v` structure (must have `constraint_id`, `violated`, `claims`, `reason`)
- Regex syntax errors (use raw strings with backticks)

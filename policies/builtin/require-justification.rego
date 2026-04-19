package rigor.builtin.require_justification

import rego.v1

# Detect functional claims with high confidence but no evidence reference.
# Catches: "This works correctly" without mentioning tests or verification.
violation contains v if {
  some c in input.claims
  c.confidence > 0.8
  regex.match(`(?i)(works|functions|correctly|successfully|operates|runs)`, c.text)
  not regex.match(`(?i)(test|tested|verified|confirmed|benchmark|measured|checked)`, c.text)
  v := {
    "constraint_id": "require-justification",
    "violated": true,
    "claims": [c.id],
    "reason": "Functional claim lacks evidence - cite tests, benchmarks, or verification"
  }
}

# Detect performance claims without measurement evidence.
# Catches: "This is fast" or "low latency" without benchmarks.
violation contains v if {
  some c in input.claims
  c.confidence > 0.8
  regex.match(`(?i)(fast|slow|efficient|performant|low latency|high throughput|optimized)`, c.text)
  not regex.match(`(?i)(benchmark|measured|profiled|timed|ms|ns|μs|ops\/sec)`, c.text)
  v := {
    "constraint_id": "require-justification",
    "violated": true,
    "claims": [c.id],
    "reason": "Performance claim lacks measurement - cite benchmarks or profiling data"
  }
}

# Detect security claims without verification evidence.
# Catches: "This is secure" without referencing audits or security testing.
violation contains v if {
  some c in input.claims
  c.confidence > 0.8
  regex.match(`(?i)(secure|safe|protected|hardened|immune|resistant)`, c.text)
  not regex.match(`(?i)(audit|pentest|security test|CVE|OWASP|reviewed|scanned)`, c.text)
  v := {
    "constraint_id": "require-justification",
    "violated": true,
    "claims": [c.id],
    "reason": "Security claim lacks evidence - cite audits, pentests, or security reviews"
  }
}

package rigor.builtin.calibrated_confidence

import rego.v1

# Detect high-confidence claims using definitive language about uncertain topics.
# Architecture, design, and future behavior are inherently uncertain.
# Catches: "This architecture definitely scales" with confidence > 0.85.
violation contains v if {
  some c in input.claims
  c.confidence > 0.85
  regex.match(`(?i)(definitely|certainly|always|guaranteed|impossible|never fails)`, c.text)
  regex.match(`(?i)(architecture|design|scale|perform|future|evolve|maintain)`, c.text)
  v := {
    "constraint_id": "calibrated-confidence",
    "violated": true,
    "claims": [c.id],
    "reason": sprintf("Overconfident claim about uncertain topic: %v (confidence: %v)", [c.text, c.confidence])
  }
}

# Detect absolute claims about system behavior without qualification.
# Catches: "This will never crash" or "always returns in under 1ms".
violation contains v if {
  some c in input.claims
  c.confidence > 0.85
  regex.match(`(?i)(never|always|every time|100%|zero chance)`, c.text)
  regex.match(`(?i)(crash|fail|error|timeout|latency|response time|memory)`, c.text)
  v := {
    "constraint_id": "calibrated-confidence",
    "violated": true,
    "claims": [c.id],
    "reason": "Absolute claim about system behavior - use probabilistic language"
  }
}

# Detect predictions about future behavior stated as facts.
# Catches: "Users will love this" or "This will handle 10x growth".
violation contains v if {
  some c in input.claims
  c.confidence > 0.85
  regex.match(`(?i)\b(will|going to)\b.*(handle|manage|support|scale to|grow|love|prefer)`, c.text)
  not regex.match(`(?i)(should|might|could|likely|probably|expected to)`, c.text)
  v := {
    "constraint_id": "calibrated-confidence",
    "violated": true,
    "claims": [c.id],
    "reason": "Future prediction stated as fact - qualify with uncertainty"
  }
}

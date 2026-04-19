package rigor.builtin.no_fabricated_apis

import rego.v1

# Detect fabricated API methods using common hallucination patterns.
# Catches: api.nonexistentMethod, lib.fabricatedFeature, etc.
violation contains v if {
  some c in input.claims
  regex.match(`(?i)\b\w+\.(magical|fabricated|nonexistent|auto_heal|doesNotExist)\b`, c.text)
  v := {
    "constraint_id": "no-fabricated-apis",
    "violated": true,
    "claims": [c.id],
    "reason": sprintf("Fabricated API method detected: %v", [c.text])
  }
}

# Detect claims about capabilities that don't exist in any known library.
# Catches: "supports self-healing", "provides magic retry", etc.
violation contains v if {
  some c in input.claims
  regex.match(`(?i)(supports|provides|enables|offers).*(magic|auto-fix|self-healing|mind-reading|telepathic)`, c.text)
  c.confidence > 0.8
  v := {
    "constraint_id": "no-fabricated-apis",
    "violated": true,
    "claims": [c.id],
    "reason": "Claimed capability does not exist"
  }
}

# Detect high-certainty feature claims without documentation references.
# Catches: "definitely has feature X" without citing docs.
violation contains v if {
  some c in input.claims
  regex.match(`(?i)(definitely|certainly|always|guaranteed).*(feature|capability|method|function)`, c.text)
  not regex.match(`(?i)(documented|reference|spec|according to|per the docs)`, c.text)
  c.confidence > 0.9
  v := {
    "constraint_id": "no-fabricated-apis",
    "violated": true,
    "claims": [c.id],
    "reason": "High-certainty feature claim without documentation reference"
  }
}

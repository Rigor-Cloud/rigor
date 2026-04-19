package rigor.constraints.no_fabricated_apis

import rego.v1

# Detect fabricated API methods
violation contains v if {
  some c in input.claims
  regex.match(`(?i)api\.(magical|fabricated|nonexistent|auto_heal)`, c.text)
  v := {
    "constraint_id": "no-fabricated-apis",
    "violated": true,
    "claims": [c.id],
    "reason": sprintf("Fabricated API method detected: %v", [c.text])
  }
}

# Detect claims about nonexistent capabilities
violation contains v if {
  some c in input.claims
  regex.match(`(?i)(supports|provides|enables).*(magic|auto-fix|self-healing|mind-reading)`, c.text)
  c.confidence > 0.8
  v := {
    "constraint_id": "no-fabricated-apis",
    "violated": true,
    "claims": [c.id],
    "reason": "Claimed capability does not exist in the library"
  }
}

# Detect claims about features with certainty but no documentation
violation contains v if {
  some c in input.claims
  regex.match(`(?i)(definitely|certainly|always).*(feature|capability|method)`, c.text)
  not regex.match(`(?i)(documented|reference|spec|according to)`, c.text)
  c.confidence > 0.9
  v := {
    "constraint_id": "no-fabricated-apis",
    "violated": true,
    "claims": [c.id],
    "reason": "High-certainty feature claim without documentation reference"
  }
}

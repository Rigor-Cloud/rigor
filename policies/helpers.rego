package rigor.helpers

# Check if any claim matches a text pattern (regex)
match_claim(claims, pattern) = matches {
    matches := [c | c := claims[_]; regex.match(pattern, c.text)]
}

# Check if a claim has a specific domain
claims_in_domain(claims, domain) = filtered {
    filtered := [c | c := claims[_]; c.domain == domain]
}

# Check if any claim text contains a substring
has_pattern(claims, substring) = found {
    found := [c | c := claims[_]; contains(c.text, substring)]
}

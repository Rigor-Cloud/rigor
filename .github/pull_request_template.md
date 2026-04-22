<!-- Thank you for contributing to Rigor! -->

# Changes

<!--
Describe your changes here. Ideally you can take this straight from a
descriptive commit message.

If this PR implements part of an epistemic-expansion plan phase, link to
the plan section (e.g. `.planning/roadmap/epistemic-expansion-plan.md` Phase 2D).
-->

# Submitter Checklist

As the author of this PR, please check off the items in this checklist:

- [ ] All new functionality has **tests** — unit tests for pure logic, integration tests for proxy or daemon behavior.
- [ ] `cargo test --all-features`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo fmt -- --check` all pass locally.
- [ ] If the change touches the constraint schema, it is backward-compatible (new fields are `Option<T>` or have serde defaults) and historical `rigor.yaml` files continue to parse.
- [ ] If the change touches DF-QuAD (`constraint/graph.rs`), the `test_dfquad_multi_attacker_product_vs_mean` regression guard at `graph.rs:447` stays green.
- [ ] If the change alters the request/response pipeline, auto-retry and PII redaction still work end-to-end.
- [ ] Docs updated for any user-facing changes (CLI flags, environment variables, rigor.yaml format, integration steps).
- [ ] Commit message follows the project convention: `<type>(<scope>): <summary>` with a body describing **why**.
- [ ] A kind label is applied (or will be requested with `/kind <type>` below). Valid types: `bug`, `cleanup`, `design`, `documentation`, `feature`, `flake`, `misc`, `question`, `rfc`.
- [ ] Release notes block below has been updated with any user-facing changes (CLI flags, new filters, dashboard tabs, schema additions). Write `NONE` if the change is purely internal.
- [ ] Release notes contain the string "action required" if the change requires users to update rigor.yaml, re-run `rigor trust`, rotate keys, or migrate data.

# Release Notes

```release-note
NONE
```

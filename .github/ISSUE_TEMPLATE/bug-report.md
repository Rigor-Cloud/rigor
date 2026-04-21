---
name: Bug Report
about: Something in Rigor is broken or behaving unexpectedly
labels: kind/bug
---

# Expected Behavior

<!-- What did you expect rigor to do? -->

# Actual Behavior

<!-- What actually happened? Include any relevant error output, dashboard screenshots, or violation log excerpts. -->

# Steps to Reproduce

1.
2.
3.

# Environment

<!-- Please fill in as much as you can. -->

- **Rigor version:**
  ```
  (paste output of `rigor --version`)
  ```

- **Agent being grounded through rigor:**  <!-- claude / opencode / codex / other -->

- **OS + architecture:**  <!-- e.g. macOS 14.5 arm64, Ubuntu 22.04 x86_64 -->

- **Rust toolchain (only if building from source):**
  ```
  (paste output of `rustc --version && cargo --version`)
  ```

# Observability snapshot

<!-- Optional but very helpful. -->

- **Relevant dashboard events:** <!-- LIVE tab screenshot, constraint that fired, etc. -->
- **Violation log excerpt:**
  ```
  (paste last few lines of ~/.rigor/violations.jsonl if relevant)
  ```
- **Daemon log excerpt:**
  ```
  (paste last relevant section of ~/.rigor/sessions/<id>/rigor.log)
  ```

# Additional Info

<!-- rigor.yaml snippet, minimal reproducing constraint, anything else. -->

---
phase: 11-e2e-coverage-gaps
plan: 01
subsystem: testing
tags: [connect, tunnel, mitm, tls, hyper-util, proxy, e2e]

# Dependency graph
requires:
  - phase: 07-crates-rigor-tests-integration-test-infrastructure
    provides: TestProxy, IsolatedHome, MockLlmServer harness primitives
provides:
  - TestProxy with HTTP CONNECT upgrade support via hyper_util serve_connection_with_upgrades
  - E2E tests for blind-tunnel (non-LLM hosts) and MITM TLS handshake (LLM hosts)
affects: [12-b1-b2-b3-integration, proxy-tests, connect-tunnel]

# Tech tracking
tech-stack:
  added: [hyper (explicit in rigor-harness), hyper-util (explicit in rigor-harness), tower (explicit in rigor-harness)]
  patterns: [hyper_util accept loop with serve_connection_with_upgrades for upgrade-capable test server, raw TCP CONNECT + TLS handshake test pattern, MITM_LOCK mutex for global AtomicBool serialization]

key-files:
  created:
    - crates/rigor/tests/connect_tunnel.rs
  modified:
    - crates/rigor-harness/src/proxy.rs
    - crates/rigor-harness/Cargo.toml

key-decisions:
  - "Upgraded TestProxy from axum::serve to hyper_util accept loop -- axum::serve does not support HTTP CONNECT upgrades"
  - "Used rcgen to parse CA PEM in tests instead of adding rustls-pemfile dependency -- reuses existing workspace deps"
  - "MITM test sends raw HTTP POST over TLS stream rather than using reqwest -- validates full MITM pipeline end-to-end"

patterns-established:
  - "CONNECT tunnel test pattern: start_echo_server + send_connect helper for blind-tunnel verification"
  - "MITM TLS test pattern: load_ca_client_config from IsolatedHome CA PEM for TLS handshake validation"
  - "hyper_util accept loop pattern for TestProxy: matches production daemon/mod.rs TLS listener"

requirements-completed: [REQ-021]

# Metrics
duration: 7min
completed: 2026-04-24
---

# Phase 11 Plan 01: CONNECT Tunnel E2E Tests Summary

**TestProxy upgraded to hyper_util serve_connection_with_upgrades; 2 E2E tests cover blind-tunnel byte passthrough and MITM TLS handshake with CA-signed cert validation**

## Performance

- **Duration:** 7 min
- **Started:** 2026-04-24T02:43:58Z
- **Completed:** 2026-04-24T02:50:58Z
- **Tasks:** 2
- **Files modified:** 3

## Accomplishments
- TestProxy now uses `hyper_util::server::conn::auto::Builder::serve_connection_with_upgrades()` instead of `axum::serve()`, enabling HTTP CONNECT upgrades
- `blind_tunnel_non_llm_host` test proves CONNECT to non-LLM host creates byte-for-byte tunnel via `copy_bidirectional`
- `mitm_tls_handshake_validates_against_ca` test proves CONNECT to LLM host results in TLS termination with valid CA-signed cert, and HTTP requests flow through the decrypted MITM tunnel to the mock LLM server

## Task Commits

Each task was committed atomically:

1. **Task 1: Upgrade TestProxy to use hyper_util serve_connection_with_upgrades** - `12dccf4` (feat)
2. **Task 2: Create connect_tunnel.rs with blind-tunnel and MITM handshake E2E tests** - `ee558eb` (test)

## Files Created/Modified
- `crates/rigor-harness/src/proxy.rs` - Replaced axum::serve with hyper_util accept loop in both start() and start_with_mock()
- `crates/rigor-harness/Cargo.toml` - Added explicit hyper, hyper-util, tower dependencies
- `crates/rigor/tests/connect_tunnel.rs` - 252-line E2E test file with blind-tunnel and MITM TLS handshake tests

## Decisions Made
- **hyper_util accept loop over axum::serve:** axum::serve does not support HTTP CONNECT upgrades (the `hyper::upgrade::on(req)` call in proxy.rs requires `serve_connection_with_upgrades`). Replaced with manual accept loop matching the production daemon/mod.rs TLS listener pattern.
- **rcgen for PEM parsing:** Used rcgen (already a dependency) to parse the CA PEM file and extract DER for the rustls root store, avoiding a new `rustls-pemfile` dependency.
- **Raw TCP + TLS for MITM test:** Used raw TCP connection + manual CONNECT + tokio_rustls TLS handshake instead of reqwest proxy config, giving direct control over each protocol layer and proving the full MITM pipeline works end-to-end.

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
- Pre-existing flaky test `score_claim_relevance_single_flight` fails intermittently when run in workspace suite (semaphore timing race) but passes when run alone. Not caused by this plan's changes.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- TestProxy now supports CONNECT upgrades, enabling Phase 12 B1/B2/B3 integration tests that need CONNECT tunnel functionality
- The `send_connect` and `load_ca_client_config` helpers in connect_tunnel.rs can be extracted to rigor-harness if future phases need them

## Self-Check: PASSED

- FOUND: crates/rigor/tests/connect_tunnel.rs
- FOUND: crates/rigor-harness/src/proxy.rs
- FOUND: commit 12dccf4
- FOUND: commit ee558eb

---
*Phase: 11-e2e-coverage-gaps*
*Completed: 2026-04-24*

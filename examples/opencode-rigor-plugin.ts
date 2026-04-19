/**
 * OpenCode ↔ Rigor plugin.
 *
 * Install:
 *   cp examples/opencode-rigor-plugin.ts .opencode/plugins/rigor.ts
 *
 * Prerequisite:
 *   rigor serve --background   (or rigor serve in another terminal)
 *
 * What it does:
 *   1. shell.env hook — injects proxy env vars so OpenCode's LLM traffic
 *      goes through the rigor daemon for constraint evaluation.
 *   2. session.created — registers the session with rigor's dashboard.
 *   3. session.idle — logs when the agent finishes a turn.
 *
 * If rigor isn't running, all hooks are no-ops. OpenCode works normally.
 */

import type { Plugin } from "@opencode-ai/plugin"

const RIGOR_HOST = process.env.RIGOR_HOST ?? "127.0.0.1"
const RIGOR_PORT = Number(process.env.RIGOR_PORT ?? 8787)
const RIGOR_BASE = `http://${RIGOR_HOST}:${RIGOR_PORT}`

// Cache health check result for 5s so we don't hit /health on every hook.
let healthCache: { alive: boolean; ts: number } | null = null

async function isRigorAlive(): Promise<boolean> {
  const now = Date.now()
  if (healthCache && now - healthCache.ts < 5000) return healthCache.alive

  let alive = false
  try {
    const res = await fetch(`${RIGOR_BASE}/health`, {
      signal: AbortSignal.timeout(250),
    })
    alive = res.ok
  } catch {
    alive = false
  }
  healthCache = { alive, ts: now }
  return alive
}

export const RigorPlugin: Plugin = async ({ client, directory }) => {
  // Log plugin load
  await client.app.log({
    body: {
      service: "rigor-plugin",
      level: "info",
      message: `Rigor plugin loaded, daemon at ${RIGOR_BASE}, project=${directory}`,
    },
  })

  return {
    // shell.env fires before every subprocess OpenCode spawns.
    // This is where we inject the proxy env vars that route LLM
    // traffic through rigor.
    "shell.env": async (_input, output) => {
      if (!(await isRigorAlive())) return

      // Proxy vars — runtimes that respect standard proxy env vars
      output.env.HTTPS_PROXY = output.env.HTTPS_PROXY || RIGOR_BASE
      output.env.HTTP_PROXY = output.env.HTTP_PROXY || RIGOR_BASE
      output.env.https_proxy = output.env.https_proxy || RIGOR_BASE
      output.env.http_proxy = output.env.http_proxy || RIGOR_BASE

      // Don't route OpenCode's internal loopback through the proxy
      output.env.NO_PROXY = output.env.NO_PROXY || "localhost,127.0.0.1,::1"
      output.env.no_proxy = output.env.no_proxy || "localhost,127.0.0.1,::1"

      // SDK-specific base URL overrides (for SDKs that ignore proxy vars)
      output.env.ANTHROPIC_BASE_URL =
        output.env.ANTHROPIC_BASE_URL || RIGOR_BASE
      output.env.OPENAI_BASE_URL = output.env.OPENAI_BASE_URL || RIGOR_BASE
      output.env.CLOUD_ML_API_ENDPOINT =
        output.env.CLOUD_ML_API_ENDPOINT || `${RIGOR_HOST}:${RIGOR_PORT}`

      // Accept rigor's self-signed MITM cert
      output.env.NODE_TLS_REJECT_UNAUTHORIZED =
        output.env.NODE_TLS_REJECT_UNAUTHORIZED ?? "0"

      // Marker so logs/tooling can tell traffic was routed
      output.env.RIGOR_ROUTED = "1"
    },

    // Fires when a new OpenCode session starts.
    // Register it with rigor so the dashboard can track it, AND tell rigor
    // which project directory we're in so it can load that project's
    // rigor.yaml and compile the right constraint set.
    "session.created": async (input) => {
      if (!(await isRigorAlive())) return

      // Register the project directory so the daemon can discover rigor.yaml
      // and hot-reload constraints for this session.
      try {
        await fetch(`${RIGOR_BASE}/api/project/register`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ directory: directory }),
          signal: AbortSignal.timeout(500),
        })
      } catch {
        // Best-effort — if the daemon can't load the project config we
        // fall back to whatever constraints it already had.
      }

      try {
        await fetch(`${RIGOR_BASE}/api/gate/register-snapshot`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({
            session_id: input.info?.id ?? "unknown",
            tool_name: "opencode.session",
            affected_paths: [],
            metadata: { source: "rigor-plugin", directory },
          }),
          signal: AbortSignal.timeout(500),
        })
      } catch {
        // Best-effort — don't block session start
      }
    },

    // Fires when the agent finishes a turn and is idle.
    "session.idle": async () => {
      if (!(await isRigorAlive())) return
      await client.app.log({
        body: {
          service: "rigor-plugin",
          level: "debug",
          message: "Session idle — agent turn complete",
        },
      })
    },
  }
}

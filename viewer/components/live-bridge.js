// ========= Live data bridge =========
// Connects to the rigor daemon's WebSocket + REST APIs and populates
// window.RIGOR_DATA in the same shape the UI components expect.
// Falls back to static seed data if the daemon isn't reachable.

(function () {
  const WS_URL = 'ws://' + window.location.host + '/ws';
  const API_BASE = window.location.origin;

  // ── Reactive state container ──
  // Components call RIGOR_DATA.subscribe(fn) to get notified on changes.
  const listeners = [];
  function notify() { listeners.forEach(fn => { try { fn(); } catch(e) { console.error(e); } }); }

  const state = {
    connected: false,
    sessionId: null,
    sources: [],
    constraints: [],
    nodes: [],
    edges: [],
    events: [],
    stream: [],
    // Per-request streamed text + violations, keyed by request_id.
    // { [request_id]: { text, status, model, violations, blockedText, feedback, durationMs } }
    streams: {},
    // Sequential request log for the LogStrip click-through.
    requestLog: [],
    // Latest ContextInjected payload (so the Context drawer tab shows what
    // rigor injected on the most recent request).
    contextInjected: null,
    // Chronological judge entries (relevance + evaluation + activity).
    judgeEntries: [],
    // Retry rows for the Retries tab.
    retries: [],
    // Pending and resolved action gates.
    actionGates: [],
    // Timeline ticks: { ts, type, request_id, latency? }
    timeline: [],
    // Latest daemon/proxy log lines (capped).
    daemonLogs: [],
    // Governance live state, populated from GovernanceState events.
    governance: { paused: false, blockNext: false },
    // Accumulated stats
    stats: { requests: 0, claims: 0, violations: 0, pii: 0, tokens: 0, constraints: 0 },
    // Cost tracking
    cost: { input_tokens: 0, output_tokens: 0, total_usd: 0.0, by_model: {} },
    // Sessions list (from REST)
    sessions: [],
  };

  function pushCapped(arr, item, cap) {
    arr.push(item);
    if (arr.length > cap) arr.splice(0, arr.length - cap);
  }

  // ── Map daemon constraint → UI constraint ──
  function mapConstraint(c) {
    return {
      id: c.id,
      label: c.name || c.id,
      scope: c.domain || c.epistemic || 'general',
      live: true,
      hits: 0,
      blocks: 0,
      epistemic: c.epistemic,
      strength: c.strength,
      baseStrength: c.baseStrength || c.base_strength,
      tags: c.tags || [],
    };
  }

  // ── Map daemon graph node → UI node ──
  function mapGraphNode(n, idx) {
    const spacing = 130;
    const cols = 4;
    return {
      id: n.id,
      type: n.type === 'constraint' ? 'source' : 'claim',
      x: 90 + (idx % cols) * spacing,
      y: 90 + Math.floor(idx / cols) * spacing,
      label: n.name || n.id,
      text: n.text || n.description || n.name || '',
      grounded: n.type === 'constraint',
      status: n.decision === 'block' ? 'block' : n.severity === 'warn' ? 'warn' : 'pass',
      epistemic: n.epistemic,
      strength: n.strength,
      nodeType: n.type,
    };
  }

  // ── Map daemon graph link → UI edge ���─
  function mapGraphEdge(l) {
    const kindMap = { supports: 'support', attacks: 'attack', undercuts: 'attack', violates: 'attack' };
    const w = l.relation === 'attacks' || l.relation === 'undercuts' || l.relation === 'violates' ? -0.6 : 0.6;
    return {
      from: l.source,
      to: l.target,
      kind: kindMap[l.relation] || 'support',
      w: w,
      excerpt: l.relation,
    };
  }

  // ── WebSocket event handler ──
  function handleEvent(evt) {
    const now = new Date();
    const eventId = 'e' + (state.events.length + 1);

    switch (evt.type) {
      case 'Request':
        state.stats.requests++;
        state.streams[evt.id] = { text: '', status: 'streaming', model: evt.model, violations: [], blockedText: '', feedback: '', durationMs: null };
        pushCapped(state.requestLog, { id: evt.id, ts: now, method: evt.method, path: evt.path, model: evt.model, status: 'streaming', durationMs: null }, 200);
        state.events.push({
          id: eventId, t: now, kind: 'request', target: evt.id,
          text: evt.method + ' ' + evt.path + ' (' + evt.model + ')', status: 'info'
        });
        break;

      case 'Response': {
        const r = state.requestLog.find(x => x.id === evt.id);
        if (r) { r.durationMs = evt.duration_ms ?? null; r.status = (evt.status >= 400 ? 'error' : (r.status === 'streaming' ? 'done' : r.status)); }
        if (state.streams[evt.id]) state.streams[evt.id].durationMs = evt.duration_ms ?? null;
        if (evt.duration_ms != null) pushCapped(state.timeline, { ts: Date.now(), type: 'latency', request_id: evt.id, latency: evt.duration_ms }, 500);
        break;
      }

      case 'ContextInjected':
        state.contextInjected = {
          request_id: evt.request_id || evt.id || null,
          original_system: evt.original_system || '',
          context_preview: evt.context_preview || '',
          constraints_count: evt.constraints_count ?? null,
          ts: now,
        };
        state.events.push({
          id: eventId, t: now, kind: 'judge',
          text: (evt.constraints_count ?? 0) + ' constraints injected',
          status: 'info',
        });
        break;

      case 'ClaimExtracted':
        state.stats.claims++;
        // Add as a claim node if not already present
        if (!state.nodes.find(n => n.id === evt.id)) {
          state.nodes.push({
            id: evt.id, type: 'claim',
            x: 360 + Math.random() * 300, y: 130 + Math.random() * 400,
            label: evt.id, text: evt.text,
            grounded: false, status: 'pass',
          });
        }
        state.events.push({
          id: eventId, t: now, kind: 'claim', target: evt.id,
          text: evt.text, status: 'pass'
        });
        break;

      case 'Violation':
        state.stats.violations++;
        // Update claim node status
        const claimNode = state.nodes.find(n => n.id === evt.claim_id);
        if (claimNode) {
          claimNode.status = evt.severity === 'block' ? 'block' : 'warn';
        }
        // Add edge from claim to constraint
        if (!state.edges.find(e => e.from === evt.claim_id && e.to === evt.constraint_id)) {
          state.edges.push({
            from: evt.claim_id, to: evt.constraint_id,
            kind: 'attack', w: -evt.strength,
            excerpt: evt.reason,
          });
        }
        // Update constraint hit count
        const cst = state.constraints.find(c => c.id === evt.constraint_id);
        if (cst) {
          cst.hits++;
          if (evt.severity === 'block') cst.blocks++;
        }
        // Attach to per-request stream so the highlighter can mark spans.
        Object.values(state.streams).forEach(s => { s.violations.push({ claim_id: evt.claim_id, constraint_id: evt.constraint_id, reason: evt.reason, severity: evt.severity }); });
        state.events.push({
          id: eventId, t: now, kind: 'claim', target: evt.claim_id,
          text: evt.reason, status: evt.severity === 'block' ? 'block' : 'warn',
          reason: evt.constraint_id + ': ' + evt.reason,
        });
        break;

      case 'Decision': {
        if (state.streams[evt.request_id]) {
          state.streams[evt.request_id].status = evt.decision === 'block' ? 'blocked' : 'allowed';
        }
        const rl = state.requestLog.find(x => x.id === evt.request_id);
        if (rl) rl.status = evt.decision === 'block' ? 'blocked' : 'allowed';
        if (evt.decision === 'block') {
          pushCapped(state.timeline, { ts: Date.now(), type: 'block', request_id: evt.request_id }, 500);
        }
        state.events.push({
          id: eventId, t: now, kind: 'judge', target: evt.request_id,
          text: 'Decision: ' + evt.decision + ' (' + evt.violations + ' violations, ' + evt.claims + ' claims)',
          status: evt.decision === 'block' ? 'block' : evt.decision === 'allow' ? 'pass' : 'info',
        });
        break;
      }

      case 'Retry': {
        const rid = evt.request_id || '';
        if (rid && state.streams[rid]) {
          if (evt.blocked_text) state.streams[rid].blockedText = evt.blocked_text;
          if (evt.feedback) state.streams[rid].feedback = evt.feedback;
        }
        if (evt.status === 'retrying') {
          state.retries.push({
            request_id: rid,
            blockedText: evt.blocked_text || '',
            feedback: evt.feedback || '',
            retryResult: '',
            status: 'retrying',
            message: evt.message || '',
            ts: now,
          });
          pushCapped(state.timeline, { ts: Date.now(), type: 'retry', request_id: rid }, 500);
        } else if (evt.status === 'retry_success' || evt.status === 'retry_failed') {
          for (let i = state.retries.length - 1; i >= 0; i--) {
            if (state.retries[i].status === 'retrying' && state.retries[i].request_id === rid) {
              state.retries[i].status = evt.status;
              state.retries[i].retryResult = evt.blocked_text || evt.message || '';
              break;
            }
          }
        }
        state.events.push({
          id: eventId, t: now, kind: 'retract', target: rid,
          text: evt.message, status: 'retract',
        });
        break;
      }

      case 'StreamText':
        state.stream.push({ text: evt.text, cls: '' });
        if (evt.request_id && state.streams[evt.request_id]) {
          state.streams[evt.request_id].text = evt.text;
        }
        break;

      case 'ClaimRelevance': {
        // Add relevance edge so the graph reflects judge output.
        const relType = evt.relevance === 'high' ? 'relevant_high' : 'relevant_medium';
        if (!state.edges.find(e => e.from === evt.claim_id && e.to === evt.constraint_id && e.kind === relType)) {
          state.edges.push({ from: evt.claim_id, to: evt.constraint_id, kind: relType, w: 0.4, excerpt: evt.reason });
        }
        pushCapped(state.judgeEntries, {
          id: eventId, t: now, kind: 'relevance',
          text: evt.claim_id + ' → ' + evt.constraint_id + ' [' + evt.relevance + '] ' + (evt.reason || ''),
        }, 200);
        break;
      }

      case 'ActionGate':
        state.actionGates.push({
          gate_id: evt.gate_id,
          action_text: evt.action_text || '',
          reason: evt.reason || '',
          status: 'pending',
          ts: now,
        });
        state.events.push({
          id: eventId, t: now, kind: 'claim', target: evt.gate_id,
          text: 'Action gate: ' + (evt.action_text || ''), status: 'warn',
          reason: evt.reason,
        });
        break;

      case 'ActionGateDecision': {
        const g = state.actionGates.find(x => x.gate_id === evt.gate_id);
        if (g) g.status = evt.approved ? 'approved' : 'rejected';
        state.events.push({
          id: eventId, t: now, kind: 'judge', target: evt.gate_id,
          text: 'Action gate ' + (evt.approved ? 'approved' : 'rejected'),
          status: evt.approved ? 'pass' : 'block',
        });
        break;
      }

      case 'GovernanceState':
        if (typeof evt.paused === 'boolean') state.governance.paused = evt.paused;
        if (typeof evt.block_next === 'boolean') state.governance.blockNext = evt.block_next;
        state.events.push({
          id: eventId, t: now, kind: 'judge',
          text: 'Governance: ' + (evt.action || '') + (evt.detail ? ' — ' + evt.detail : ''),
          status: 'info',
        });
        break;

      case 'DaemonLog':
      case 'ProxyLog':
        pushCapped(state.daemonLogs, { id: eventId, t: now, level: evt.level || 'info', source: evt.type === 'ProxyLog' ? 'proxy' : 'daemon', text: evt.message || evt.text || '' }, 200);
        break;

      case 'ChatResponse':
      case 'ClaudeCodeEvent':
        // Surface as a generic info row in the daemon log lane.
        pushCapped(state.daemonLogs, { id: eventId, t: now, level: 'info', source: evt.type, text: evt.detail || JSON.stringify(evt) }, 200);
        break;

      case 'PiiDetected':
        state.stats.pii++;
        state.events.push({
          id: eventId, t: now, kind: 'claim', target: evt.request_id,
          text: 'PII detected: ' + evt.pii_type + ' (' + evt.action + ')',
          status: evt.action === 'block' ? 'block' : 'warn',
        });
        break;

      case 'TokenUsage':
        state.stats.tokens += evt.input_tokens + evt.output_tokens;
        state.cost.input_tokens += evt.input_tokens;
        state.cost.output_tokens += evt.output_tokens;
        break;

      case 'SessionCost':
        state.cost.total_usd = evt.total_cost_usd;
        state.sessionId = evt.session_id;
        break;

      case 'JudgeActivity':
      case 'JudgeEvaluation':
        state.events.push({
          id: eventId, t: now, kind: 'judge',
          text: evt.detail || evt.eval_type || evt.action,
          status: 'info',
        });
        break;

      case 'GovernanceState':
        state.events.push({
          id: eventId, t: now, kind: 'judge',
          text: 'Governance: ' + evt.action + ' — ' + evt.detail,
          status: 'info',
        });
        break;

      case 'ActionGate':
        state.events.push({
          id: eventId, t: now, kind: 'claim', target: evt.gate_id,
          text: 'Action gate: ' + evt.action_text, status: 'warn',
          reason: evt.reason,
        });
        break;
    }

    notify();
  }

  // ── WebSocket connection with auto-reconnect ──
  let ws = null;
  let reconnectTimer = null;

  function connect() {
    try {
      ws = new WebSocket(WS_URL);

      ws.onopen = () => {
        state.connected = true;
        notify();
        console.log('[rigor] WebSocket connected');
      };

      ws.onmessage = (msg) => {
        try {
          const evt = JSON.parse(msg.data);
          handleEvent(evt);
        } catch (e) {
          console.warn('[rigor] bad WS message:', e);
        }
      };

      ws.onclose = () => {
        state.connected = false;
        notify();
        console.log('[rigor] WebSocket closed, reconnecting in 3s...');
        reconnectTimer = setTimeout(connect, 3000);
      };

      ws.onerror = () => {
        ws.close();
      };
    } catch (e) {
      console.warn('[rigor] WebSocket failed:', e);
      reconnectTimer = setTimeout(connect, 5000);
    }
  }

  // ── REST API loaders ──
  async function loadGraph() {
    try {
      const resp = await fetch(API_BASE + '/graph.json');
      if (!resp.ok) return;
      const data = await resp.json();
      state.nodes = (data.nodes || []).map(mapGraphNode);
      state.edges = (data.links || []).map(mapGraphEdge);
      state.constraints = (data.nodes || [])
        .filter(n => n.type === 'constraint')
        .map(mapConstraint);
      state.stats.constraints = state.constraints.length;

      // Build sources from constraints' domains
      const domains = new Map();
      state.constraints.forEach(c => {
        const d = c.scope || 'general';
        if (!domains.has(d)) domains.set(d, { id: d, kind: 'YAML', label: d, n: 0 });
        domains.get(d).n++;
      });
      state.sources = Array.from(domains.values());

      notify();
    } catch (e) {
      console.warn('[rigor] failed to load graph.json:', e);
    }
  }

  async function loadSessions() {
    try {
      const resp = await fetch(API_BASE + '/api/sessions');
      if (!resp.ok) return;
      state.sessions = await resp.json();
      notify();
    } catch (e) {
      // daemon may not be running — OK
    }
  }

  async function loadViolations() {
    // Historical violations are intentionally NOT merged into state.events.
    // The live event log shows ONLY live WebSocket activity from the current
    // daemon session; otherwise old entries from prior sessions render with
    // huge "9893m ago" timestamps and dominate the "live" feel.
    //
    // Pages that want history (SearchPage, ObservabilityPage) fetch
    // /api/violations directly with their own filter params.
    try {
      const resp = await fetch(API_BASE + '/api/violations');
      if (!resp.ok) return;
      const violations = await resp.json();
      state.stats.violations = Math.max(state.stats.violations, (violations || []).length);
      notify();
    } catch (e) {
      // OK
    }
  }

  // ── Initialize ──
  // Load REST data, then connect WebSocket for live updates
  Promise.all([loadGraph(), loadSessions(), loadViolations()]).then(() => {
    connect();
  });

  // ── Expose to UI ─��
  window.RIGOR_DATA = new Proxy(state, {
    get(target, prop) {
      if (prop === 'subscribe') return (fn) => { listeners.push(fn); return () => { const i = listeners.indexOf(fn); if (i >= 0) listeners.splice(i, 1); }; };
      if (prop === 'refresh') return () => Promise.all([loadGraph(), loadSessions(), loadViolations()]);
      return target[prop];
    }
  });
})();

// ========= Tiny utility =========
window.fmtTime = function (d) {
  if (!(d instanceof Date)) d = new Date(d);
  const dd = (n) => String(n).padStart(2, '0');
  return `${dd(d.getHours())}:${dd(d.getMinutes())}:${dd(d.getSeconds())}`;
};
window.relTime = function (d) {
  if (!(d instanceof Date)) d = new Date(d);
  const s = Math.max(1, Math.round((Date.now() - d.getTime()) / 1000));
  if (s < 60) return s + 's ago';
  return Math.round(s / 60) + 'm ago';
};

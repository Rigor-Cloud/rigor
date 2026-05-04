/* global React, RigorBaseComponents, RigorObsCharts1, RigorObsCharts2, RigorObsCharts3, RigorObsCharts4 */
const { useState, useEffect } = React;
const { I } = RigorBaseComponents;
const { StackedHealthChart, LatencyChart, ThroughputChart, ModelSparkGrid } = RigorObsCharts1;
const { ClaimsHistogram, StrengthDistribution, TimeToFirstBlock } = RigorObsCharts2;
const { ConstraintHeatmap, CooccurrenceMatrix, RecallFpScatter } = RigorObsCharts3;
const { SourceSankey, CoverageMap, RetractFunnel, SessionStrips } = RigorObsCharts4;

// Format an ISO timestamp as HH:MM:SS, or '—' if missing/invalid.
function fmtTs(iso) {
  if (!iso) return '—';
  const d = new Date(iso);
  if (isNaN(d.getTime())) return '—';
  const dd = (n) => String(n).padStart(2, '0');
  return `${dd(d.getHours())}:${dd(d.getMinutes())}:${dd(d.getSeconds())}`;
}

// ====== Observability ======
function ObservabilityPage() {
  const [evalStats, setEvalStats] = useState(null);
  const [costStats, setCostStats] = useState(null);
  const [sessionList, setSessionList] = useState(null);

  useEffect(() => {
    fetch('/api/eval').then(r => r.ok ? r.json() : null).then(setEvalStats).catch(() => {});
    fetch('/api/cost').then(r => r.ok ? r.json() : null).then(setCostStats).catch(() => {});
    fetch('/api/sessions').then(r => r.ok ? r.json() : null).then(setSessionList).catch(() => {});
  }, []);

  const totalReq = (sessionList || []).reduce((a, s) => a + (s.requests || 0), 0);
  const totalViol = evalStats?.total_violations ?? 0;
  const totalSess = evalStats?.total_sessions ?? 0;
  const fpCount = evalStats?.false_positive_count ?? 0;
  const precisionPct = (evalStats?.precision ?? 0).toFixed(1);
  const violPerSess = (evalStats?.violations_per_session ?? 0).toFixed(1);
  const totalCost = (costStats?.total_cost_usd ?? 0).toFixed(2);

  return (
    <div className="page-scroll">
      <h1 className="page-h1">Observability</h1>
      <p className="page-sub">All sessions running through the local proxy.</p>

      <div className="kpi-grid">
        <div className="kpi-tile"><div className="kt-label">requests</div><div className="kt-value">{totalReq.toLocaleString()}</div><div className="kt-meta">across {sessionList?.length ?? 0} sessions</div></div>
        <div className="kpi-tile"><div className="kt-label">sessions</div><div className="kt-value">{totalSess}</div><div className="kt-meta">{violPerSess} violations / session</div></div>
        <div className="kpi-tile bad"><div className="kt-label">violations</div><div className="kt-value">{totalViol.toLocaleString()}</div><div className="kt-meta">{evalStats?.constraints?.length ?? 0} constraints fired</div></div>
        <div className="kpi-tile"><div className="kt-label">false positives</div><div className="kt-value">{fpCount}</div><div className="kt-meta">annotated as FP</div></div>
        <div className="kpi-tile good"><div className="kt-label">precision</div><div className="kt-value">{precisionPct}%</div><div className="kt-meta">on annotated violations</div></div>
        <div className="kpi-tile"><div className="kt-label">cost</div><div className="kt-value">${totalCost}</div><div className="kt-meta">{(costStats?.total_input_tokens ?? 0).toLocaleString()} in / {(costStats?.total_output_tokens ?? 0).toLocaleString()} out</div></div>
      </div>

      {/* ─── Time-series ─── */}
      <div className="section-head">
        <div className="section-h">Time-series</div>
        <span className="section-meta">verdict mix · latency · throughput · per-model</span>
      </div>
      <div className="chart-grid">
        <StackedHealthChart/>
        <LatencyChart/>
        <ThroughputChart/>
        <ModelSparkGrid/>
      </div>

      {/* ─── Distributions ─── */}
      <div className="section-head">
        <div className="section-h">Distributions</div>
        <span className="section-meta">claim shape · strength · time-to-first-block</span>
      </div>
      <div className="chart-grid">
        <ClaimsHistogram/>
        <StrengthDistribution/>
        <TimeToFirstBlock/>
      </div>

      {/* ─── Constraint analytics ─── */}
      <div className="section-head">
        <div className="section-h">Constraint analytics</div>
        <span className="section-meta">drift · co-occurrence · tuning</span>
      </div>
      <div className="chart-grid">
        <ConstraintHeatmap/>
        <CooccurrenceMatrix/>
        <RecallFpScatter/>
      </div>

      {/* ─── Source / grounding ─── */}
      <div className="section-head">
        <div className="section-h">Source &amp; grounding</div>
        <span className="section-meta">where evidence comes from · how much lands</span>
      </div>
      <div className="chart-grid">
        <SourceSankey/>
        <CoverageMap/>
      </div>

      {/* ─── Outcomes ─── */}
      <div className="section-head">
        <div className="section-h">Outcomes</div>
        <span className="section-meta">retract success · session timelines</span>
      </div>
      <div className="chart-grid">
        <RetractFunnel/>
        <SessionStrips/>
      </div>

      <div className="section-head">
        <div className="section-h">Sessions</div>
        <span className="section-meta">last 24h · click to drill in</span>
      </div>
      <table className="table">
        <thead><tr><th>session</th><th>started</th><th>agent</th><th>requests</th><th>violations</th><th>constraints</th><th>status</th></tr></thead>
        <tbody>
          {(sessionList || []).map((s, i) => {
            const st = s.alive ? 'live' : s.exit_code != null && s.exit_code !== 0 ? 'flagged' : 'done';
            return (
              <tr key={s.id || i}>
                <td className="mono name">{(s.name || s.id || '').slice(0, 24)}</td>
                <td className="mono dim">{fmtTs(s.started_at)}</td>
                <td className="mono">{s.agent || '—'}</td>
                <td>{s.requests ?? 0}</td>
                <td className="mono" style={{color: (s.violations || 0) > 0 ? 'var(--signal-violate)' : 'var(--ink-3)'}}>{s.violations ?? 0}</td>
                <td className="mono dim">{s.constraints ?? 0}</td>
                <td><span className={'badge ' + (st==='live'?'badge-warn':st==='flagged'?'badge-violate':'badge-neutral')}>{st}</span></td>
              </tr>
            );
          })}
          {(sessionList || []).length === 0 && (
            <tr><td colSpan="7" style={{textAlign:'center', color:'var(--ink-3)', padding:'24px'}}>No sessions yet — start one with <code>rigor ground</code>.</td></tr>
          )}
        </tbody>
      </table>

      <div className="section-head">
        <div className="section-h">Top constraints by hit count</div>
        <span className="section-meta">from violation log</span>
      </div>
      <table className="table">
        <thead><tr><th>id</th><th>constraint</th><th>hits</th><th>false positives</th><th>fp rate</th><th>last fired</th></tr></thead>
        <tbody>
          {(evalStats?.constraints || []).slice(0, 10).map((c, i) => (
            <tr key={c.id || i}>
              <td className="mono name">{c.id}</td>
              <td>{c.name}</td>
              <td>{c.hits}</td>
              <td className="mono" style={{color: c.false_positives > 0 ? 'var(--signal-violate)' : 'var(--ink-3)'}}>{c.false_positives}</td>
              <td>
                <div style={{display:'flex',alignItems:'center',gap:8}}>
                  <div className={'bar' + (c.fp_rate > 20 ? ' bad' : '')}><span style={{width: `${Math.min(100, c.fp_rate)}%`}}/></div>
                  <span className="mono dim">{c.fp_rate.toFixed(1)}%</span>
                </div>
              </td>
              <td className="mono dim">{fmtTs(c.last_fired)}</td>
            </tr>
          ))}
          {(evalStats?.constraints || []).length === 0 && (
            <tr><td colSpan="6" style={{textAlign:'center', color:'var(--ink-3)', padding:'24px'}}>No violations logged yet.</td></tr>
          )}
        </tbody>
      </table>
    </div>
  );
}

// ====== Violations search ======
function SearchPage() {
  const [q, setQ] = useState('');
  const [sev, setSev] = useState('');
  const [items, setItems] = useState([]);

  useEffect(() => {
    const params = new URLSearchParams();
    if (q) params.set('q', q);
    if (sev) params.set('severity', sev);
    params.set('limit', '200');
    fetch('/api/violations?' + params.toString())
      .then(r => r.ok ? r.json() : [])
      .then(rows => setItems(rows || []))
      .catch(() => setItems([]));
  }, [q, sev]);

  return (
    <div className="page-scroll">
      <h1 className="page-h1">Violations</h1>
      <p className="page-sub">Search the full archive of constraint hits.</p>
      <div className="search-bar">
        <input placeholder="claim text, constraint id, message…" value={q} onChange={e=>setQ(e.target.value)}/>
        <select value={sev} onChange={e=>setSev(e.target.value)}>
          <option value="">all severities</option>
          <option value="block">block</option>
          <option value="warn">warn</option>
        </select>
        <button className="btn btn-primary btn-sm">{I.search} Search</button>
      </div>
      <table className="table">
        <thead><tr><th>constraint</th><th>time</th><th>session</th><th>model</th><th>flagged claim</th><th>reason</th></tr></thead>
        <tbody>
          {items.map((it, i) => (
            <tr key={i}>
              <td className="mono"><span className={'badge ' + (it.severity === 'block' ? 'badge-violate' : 'badge-warn')}>{it.constraint_id}</span></td>
              <td className="mono dim">{fmtTs(it.timestamp)}</td>
              <td className="mono name">{(it.session_id || '').slice(0, 12)}</td>
              <td className="mono dim">{it.model || '—'}</td>
              <td style={{fontFamily:'var(--font-serif)',fontSize:14,maxWidth:340}}>{(it.claim_text || []).map(c => `“${c}”`).join(' ')}</td>
              <td className="dim" style={{fontSize:12}}>{it.message}</td>
            </tr>
          ))}
          {items.length === 0 && (
            <tr><td colSpan="6" style={{textAlign:'center', color:'var(--ink-3)', padding:'24px'}}>No matching violations.</td></tr>
          )}
        </tbody>
      </table>
    </div>
  );
}

// ====== Eval ======
function EvalPage() {
  const [stats, setStats] = useState(null);
  useEffect(() => {
    fetch('/api/eval').then(r => r.ok ? r.json() : null).then(setStats).catch(() => {});
  }, []);

  const total = stats?.total_violations ?? 0;
  const fp = stats?.false_positive_count ?? 0;
  const sessions = stats?.total_sessions ?? 0;
  const precision = stats?.precision ?? 0;
  const violPerSess = stats?.violations_per_session ?? 0;
  const constraints = stats?.constraints || [];

  return (
    <div className="page-scroll">
      <h1 className="page-h1">Eval</h1>
      <p className="page-sub">Constraint effectiveness against the local violation log. Annotate via <code style={{fontFamily:'var(--font-mono)',fontSize:12,padding:'1px 5px',border:'1px solid var(--rule)',borderRadius:3,background:'var(--paper-2)'}}>rigor log annotate --false-positive</code> to populate precision.</p>
      <div className="kpi-grid">
        <div className="kpi-tile good"><div className="kt-label">precision</div><div className="kt-value">{precision.toFixed(1)}%</div><div className="kt-meta">on annotated entries</div></div>
        <div className="kpi-tile"><div className="kt-label">total violations</div><div className="kt-value">{total.toLocaleString()}</div><div className="kt-meta">{constraints.length} constraints fired</div></div>
        <div className="kpi-tile bad"><div className="kt-label">false positives</div><div className="kt-value">{fp}</div><div className="kt-meta">marked via annotate</div></div>
        <div className="kpi-tile"><div className="kt-label">sessions</div><div className="kt-value">{sessions}</div><div className="kt-meta">{violPerSess.toFixed(1)} violations / session</div></div>
      </div>

      <div className="section-head"><div className="section-h">Per-constraint performance</div></div>
      <table className="table">
        <thead><tr><th>constraint</th><th>name</th><th>hits</th><th>false positives</th><th>fp rate</th><th>last fired</th></tr></thead>
        <tbody>
          {constraints.map((c, i) => (
            <tr key={c.id || i}>
              <td className="mono name">{c.id}</td>
              <td>{c.name}</td>
              <td className="mono">{c.hits}</td>
              <td className="mono" style={{color: c.false_positives > 0 ? 'var(--signal-violate)' : 'var(--ink-3)'}}>{c.false_positives}</td>
              <td className="mono" style={{color: c.fp_rate > 20 ? 'var(--signal-warn-2)' : 'var(--ink-3)'}}>{c.fp_rate.toFixed(1)}%</td>
              <td className="mono dim">{fmtTs(c.last_fired)}</td>
            </tr>
          ))}
          {constraints.length === 0 && (
            <tr><td colSpan="6" style={{textAlign:'center', color:'var(--ink-3)', padding:'24px'}}>No violations logged yet.</td></tr>
          )}
        </tbody>
      </table>
    </div>
  );
}

// ====== Constraints (catalog/editor) ======
function ConstraintsPage() {
  const rigor = useRigorData();
  const [govList, setGovList] = useState(null);
  const [filter, setFilter] = useState('');

  const refresh = () => {
    fetch('/api/governance/constraints').then(r => r.ok ? r.json() : null).then(setGovList).catch(() => {});
  };
  useEffect(() => { refresh(); }, []);

  const toggleLive = (id) => {
    fetch('/api/governance/constraints/' + encodeURIComponent(id) + '/toggle', { method: 'POST' })
      .then(refresh)
      .catch(() => {});
  };

  // Prefer governance API for live state; fall back to /graph.json constraints.
  const rows = (govList || rigor.constraints || []).map(c => ({
    id: c.id,
    label: c.label || c.name || c.id,
    scope: c.scope || c.domain || 'general',
    epistemic: c.epistemic || c.epistemic_type || '—',
    live: c.live !== false,
  })).filter(c => !filter || (c.id + c.label + c.scope).toLowerCase().includes(filter.toLowerCase()));

  return (
    <div className="page-scroll">
      <h1 className="page-h1">Constraints</h1>
      <p className="page-sub">The compiled constraint set this proxy enforces. Click a toggle to enable / disable live.</p>
      <div className="search-bar">
        <input placeholder="Filter by id, label, scope…" value={filter} onChange={e=>setFilter(e.target.value)}/>
        <button className="btn btn-secondary btn-sm" onClick={refresh}>Refresh</button>
      </div>
      <table className="table">
        <thead><tr><th>id</th><th>label</th><th>scope</th><th>epistemic</th><th>live</th></tr></thead>
        <tbody>
          {rows.map(c => (
            <tr key={c.id}>
              <td className="mono name">{c.id}</td>
              <td>{c.label}</td>
              <td className="mono dim">{c.scope}</td>
              <td className="mono dim">{c.epistemic}</td>
              <td>
                <span className={'toggle' + (c.live ? ' on' : '')}
                      style={{display:'inline-block',verticalAlign:'middle',cursor:'pointer'}}
                      onClick={() => toggleLive(c.id)}/>
              </td>
            </tr>
          ))}
          {rows.length === 0 && (
            <tr><td colSpan="5" style={{textAlign:'center', color:'var(--ink-3)', padding:'24px'}}>No constraints loaded.</td></tr>
          )}
        </tbody>
      </table>
    </div>
  );
}

window.RigorPages = { ObservabilityPage, SearchPage, EvalPage, ConstraintsPage };

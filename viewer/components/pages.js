/* global React, RigorBaseComponents, RigorObsCharts1, RigorObsCharts2, RigorObsCharts3, RigorObsCharts4 */
const { useState } = React;
const { I } = RigorBaseComponents;
const { StackedHealthChart, LatencyChart, ThroughputChart, ModelSparkGrid } = RigorObsCharts1;
const { ClaimsHistogram, StrengthDistribution, TimeToFirstBlock } = RigorObsCharts2;
const { ConstraintHeatmap, CooccurrenceMatrix, RecallFpScatter } = RigorObsCharts3;
const { SourceSankey, CoverageMap, RetractFunnel, SessionStrips } = RigorObsCharts4;

// ====== Observability ======
function ObservabilityPage() {
  return (
    <div className="page-scroll">
      <h1 className="page-h1">Observability</h1>
      <p className="page-sub">All sessions running through the local proxy. Aggregated 24h.</p>

      <div className="kpi-grid">
        <div className="kpi-tile"><div className="kt-label">requests</div><div className="kt-value">12,418</div><div className="kt-meta">+8.2% vs 7d</div></div>
        <div className="kpi-tile"><div className="kt-label">claims judged</div><div className="kt-value">38,902</div><div className="kt-meta">3.1 per response</div></div>
        <div className="kpi-tile bad"><div className="kt-label">blocks</div><div className="kt-value">214</div><div className="kt-meta">1.7% of responses</div></div>
        <div className="kpi-tile"><div className="kt-label">retracts</div><div className="kt-value">98</div><div className="kt-meta">46% of blocks recovered</div></div>
        <div className="kpi-tile good"><div className="kt-label">grounded rate</div><div className="kt-value">81.4%</div><div className="kt-meta">+2.1pp wow</div></div>
        <div className="kpi-tile"><div className="kt-label">judge p95</div><div className="kt-value">141<span style={{fontSize:13,color:'var(--ink-3)',marginLeft:3}}>ms</span></div><div className="kt-meta">haiku-4-5</div></div>
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
        <thead><tr><th>session</th><th>started</th><th>model</th><th>claims</th><th>blocks</th><th>grounded</th><th>status</th></tr></thead>
        <tbody>
          {[
            ['sess_8b3a91','14:08:03','claude-sonnet-4',  9, 2, 0.71, 'live'],
            ['sess_8b3a82','13:46:11','claude-sonnet-4', 14, 0, 0.93, 'done'],
            ['sess_8b3a74','13:31:55','gpt-4o',          22, 3, 0.68, 'done'],
            ['sess_8b3a61','13:02:18','claude-haiku-4',  6, 1, 0.83, 'done'],
            ['sess_8b3a4f','12:48:09','claude-sonnet-4', 18, 0, 0.94, 'done'],
            ['sess_8b3a3a','12:21:44','gpt-4o',         11, 4, 0.55, 'flagged'],
          ].map(([id, ts, m, c, b, g, st], i) => (
            <tr key={i}>
              <td className="mono name">{id}</td>
              <td className="mono dim">{ts}</td>
              <td className="mono">{m}</td>
              <td>{c}</td>
              <td className="mono" style={{color: b > 0 ? 'var(--signal-violate)' : 'var(--ink-3)'}}>{b}</td>
              <td>
                <div style={{display:'flex',alignItems:'center',gap:8}}>
                  <div className={'bar' + (g < 0.7 ? ' bad' : '')}><span style={{width: `${g*100}%`}}/></div>
                  <span className="mono dim">{Math.round(g*100)}%</span>
                </div>
              </td>
              <td><span className={'badge ' + (st==='live'?'badge-warn':st==='flagged'?'badge-violate':'badge-neutral')}>{st}</span></td>
            </tr>
          ))}
        </tbody>
      </table>

      <div className="section-head">
        <div className="section-h">Top constraints by hit count</div>
        <span className="section-meta">live + retired</span>
      </div>
      <table className="table">
        <thead><tr><th>id</th><th>constraint</th><th>scope</th><th>hits</th><th>blocks</th><th>recall</th></tr></thead>
        <tbody>
          {[
            ['C-1041','Numeric claim must cite source','finance', 184, 41, 0.92],
            ['C-2003','No forward-looking projections','finance', 96, 88, 0.87],
            ['C-1305','Distinguish opinion vs fact','editorial', 71, 4, 0.74],
            ['C-1102','Quote attribution required','editorial', 58, 12, 0.83],
            ['C-1133','PII redaction','safety', 22, 22, 0.99],
          ].map((r,i) => (
            <tr key={i}>
              <td className="mono name">{r[0]}</td>
              <td>{r[1]}</td>
              <td className="mono dim">{r[2]}</td>
              <td>{r[3]}</td>
              <td className="mono" style={{color: r[4]>0?'var(--signal-violate)':''}}>{r[4]}</td>
              <td>
                <div style={{display:'flex',alignItems:'center',gap:8}}>
                  <div className="bar"><span style={{width: `${r[5]*100}%`}}/></div>
                  <span className="mono dim">{Math.round(r[5]*100)}%</span>
                </div>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

// ====== Violations search ======
function SearchPage() {
  const [q, setQ] = useState('');
  const items = [
    { c: 'C-2003', t: '14:08:42', sess: 'sess_8b3a91', claim: 'Acme will outperform peers.', model: 'claude-sonnet-4', reason: 'Forward-looking projection without disclaimer.' },
    { c: 'C-2003', t: '14:08:14', sess: 'sess_8b3a91', claim: 'Margins will expand further next year.', model: 'claude-sonnet-4', reason: 'Forward-looking projection.' },
    { c: 'C-1305', t: '14:08:09', sess: 'sess_8b3a91', claim: 'The strongest quarter on record.', model: 'claude-sonnet-4', reason: 'Opinion presented as fact; source contradicts.' },
    { c: 'C-1041', t: '13:32:11', sess: 'sess_8b3a74', claim: 'AWS revenue grew roughly 19%.', model: 'gpt-4o', reason: 'Numeric claim without source citation.' },
    { c: 'C-1133', t: '12:22:07', sess: 'sess_8b3a3a', claim: 'Reach me at jane@acme.com.', model: 'gpt-4o', reason: 'Email PII detected; redacted.' },
    { c: 'C-1102', t: '12:21:55', sess: 'sess_8b3a3a', claim: '"AI will transform finance," she said.', model: 'gpt-4o', reason: 'Quote attribution missing.' },
  ].filter(x => !q || (x.c+x.claim+x.reason+x.sess+x.model).toLowerCase().includes(q.toLowerCase()));
  return (
    <div className="page-scroll">
      <h1 className="page-h1">Violations</h1>
      <p className="page-sub">Search the full archive of constraint hits.</p>
      <div className="search-bar">
        <input placeholder="claim text, constraint id, session…" value={q} onChange={e=>setQ(e.target.value)}/>
        <select><option>all constraints</option><option>finance</option><option>editorial</option><option>safety</option></select>
        <select><option>all severities</option><option>block</option><option>warn</option></select>
        <button className="btn btn-primary btn-sm">{I.search} Search</button>
      </div>
      <table className="table">
        <thead><tr><th>constraint</th><th>time</th><th>session</th><th>model</th><th>flagged claim</th><th>reason</th></tr></thead>
        <tbody>
          {items.map((it, i) => (
            <tr key={i}>
              <td className="mono"><span className="badge badge-violate">{it.c}</span></td>
              <td className="mono dim">{it.t}</td>
              <td className="mono name">{it.sess}</td>
              <td className="mono dim">{it.model}</td>
              <td style={{fontFamily:'var(--font-serif)',fontSize:14,maxWidth:340}}>“{it.claim}”</td>
              <td className="dim" style={{fontSize:12}}>{it.reason}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

// ====== Eval ======
function EvalPage() {
  return (
    <div className="page-scroll">
      <h1 className="page-h1">Eval</h1>
      <p className="page-sub">Offline scoring of judge against gold-labeled set <code style={{fontFamily:'var(--font-mono)',fontSize:12,padding:'1px 5px',border:'1px solid var(--rule)',borderRadius:3,background:'var(--paper-2)'}}>finance-grounding-v3</code>.</p>
      <div className="kpi-grid">
        <div className="kpi-tile good"><div className="kt-label">precision</div><div className="kt-value">0.91</div><div className="kt-meta">↑ 0.04 vs prev</div></div>
        <div className="kpi-tile good"><div className="kt-label">recall</div><div className="kt-value">0.86</div><div className="kt-meta">↑ 0.02</div></div>
        <div className="kpi-tile"><div className="kt-label">F1</div><div className="kt-value">0.88</div><div className="kt-meta">−</div></div>
        <div className="kpi-tile bad"><div className="kt-label">false positives</div><div className="kt-value">14</div><div className="kt-meta">of 312</div></div>
        <div className="kpi-tile"><div className="kt-label">judged</div><div className="kt-value">312</div><div className="kt-meta">3.4s mean</div></div>
        <div className="kpi-tile"><div className="kt-label">cost</div><div className="kt-value">$1.84</div><div className="kt-meta">$0.0059/item</div></div>
      </div>

      <div className="section-head"><div className="section-h">Per-constraint performance</div></div>
      <table className="table">
        <thead><tr><th>constraint</th><th>P</th><th>R</th><th>F1</th><th>FP</th><th>FN</th></tr></thead>
        <tbody>
          {[
            ['C-1041',0.94,0.91,0.92, 4, 6],
            ['C-2003',0.89,0.93,0.91, 7, 4],
            ['C-1305',0.81,0.74,0.77, 11, 14],
            ['C-1102',0.92,0.79,0.85, 3, 12],
            ['C-1133',0.99,0.99,0.99, 1, 1],
          ].map((r,i) => (
            <tr key={i}>
              <td className="mono name">{r[0]}</td>
              <td className="mono">{r[1].toFixed(2)}</td>
              <td className="mono">{r[2].toFixed(2)}</td>
              <td className="mono">{r[3].toFixed(2)}</td>
              <td className="mono" style={{color:'var(--signal-violate)'}}>{r[4]}</td>
              <td className="mono" style={{color:'var(--signal-warn-2)'}}>{r[5]}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

// ====== Constraints (catalog/editor) ======
function ConstraintsPage() {
  return (
    <div className="page-scroll">
      <h1 className="page-h1">Constraints</h1>
      <p className="page-sub">The compiled constraint set this proxy enforces. Edit, retire, or roll forward.</p>
      <div className="search-bar">
        <input placeholder="Filter by id, label, scope…" />
        <select><option>all scopes</option><option>finance</option><option>editorial</option><option>safety</option></select>
        <button className="btn btn-primary btn-sm">+ New constraint</button>
      </div>
      <table className="table">
        <thead><tr><th>id</th><th>label</th><th>scope</th><th>severity</th><th>live</th><th>last edit</th></tr></thead>
        <tbody>
          {RIGOR_DATA.constraints.map(c => (
            <tr key={c.id}>
              <td className="mono name">{c.id}</td>
              <td>{c.label}</td>
              <td className="mono dim">{c.scope}</td>
              <td><span className={'badge ' + (c.scope==='safety'?'badge-violate':c.scope==='finance'?'badge-warn':'badge-neutral')}>
                {c.scope==='safety'?'block':c.scope==='finance'?'warn+block':'warn'}
              </span></td>
              <td><span className={'toggle' + (c.live ? ' on' : '')} style={{display:'inline-block',verticalAlign:'middle'}}/></td>
              <td className="mono dim">2 days ago · jdoe</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

window.RigorPages = { ObservabilityPage, SearchPage, EvalPage, ConstraintsPage };

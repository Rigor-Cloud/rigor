/* global React */
const { useEffect, useMemo, useState, useRef } = React;

// ====== Icons ======
const I = {
  pause: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><rect x="6" y="5" width="4" height="14"/><rect x="14" y="5" width="4" height="14"/></svg>,
  play:  <svg viewBox="0 0 24 24" fill="currentColor"><polygon points="6,4 20,12 6,20"/></svg>,
  copy:  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15V5a2 2 0 0 1 2-2h10"/></svg>,
  download: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M12 4v12"/><path d="M6 12l6 6 6-6"/><path d="M4 20h16"/></svg>,
  search: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><circle cx="11" cy="11" r="7"/><path d="m21 21-4-4"/></svg>,
  filter: <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M3 5h18l-7 9v6l-4-2v-4z"/></svg>,
  more:   <svg viewBox="0 0 24 24" fill="currentColor"><circle cx="6" cy="12" r="1.6"/><circle cx="12" cy="12" r="1.6"/><circle cx="18" cy="12" r="1.6"/></svg>,
  warn:   <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M12 4l10 16H2z"/><path d="M12 10v5"/><circle cx="12" cy="18" r=".8" fill="currentColor"/></svg>,
};

// ====== Topbar ======
function Topbar({ proxy }) {
  return (
    <header className="topbar">
      <svg className="tb-logo" viewBox="0 0 32 32" fill="none" aria-label="Rigor">
        <line x1="16" y1="16" x2="26" y2="6" stroke="#B43A2E" strokeWidth="1.6" strokeLinecap="round"/>
        <circle cx="26" cy="6" r="2.4" fill="#B43A2E"/>
        <line x1="16" y1="16" x2="6" y2="26" stroke="#2E5A7A" strokeWidth="1.6" strokeLinecap="round"/>
        <circle cx="6" cy="26" r="2.4" fill="#2E5A7A"/>
        <circle cx="16" cy="16" r="5" fill="#1A1916"/>
        <circle cx="16" cy="16" r="1.6" fill="#F4F1EA"/>
      </svg>
      <span className="tb-wordmark">Rigor</span>
      <span className="tb-crumb tb-crumb-active">live</span>
      <span className="tb-sep">/</span>
      <span className="tb-crumb">{RIGOR_DATA.sessionId}</span>
      <div className="tb-spacer"/>
      <span className="proxy-pill">
        <span className="proxy-dot"/>proxy <span className="mono">localhost:7331</span>
      </span>
      <div className="kpi-strip">
        <div className="kpi"><span className="kpi-label">throughput</span><span className="kpi-value">{proxy.tput}/s</span></div>
        <div className="kpi"><span className="kpi-label">p95</span><span className="kpi-value">{proxy.p95}ms</span></div>
        <div className="kpi"><span className="kpi-label">block</span><span className="kpi-value block">{proxy.blocks}</span></div>
      </div>
    </header>
  );
}

// ====== Page Tabs ======
function PageTabs({ page, setPage, blocks }) {
  const tabs = [
    { id: 'live', label: 'Live' },
    { id: 'obs',  label: 'Observability' },
    { id: 'search', label: 'Violations', count: blocks },
    { id: 'eval', label: 'Eval' },
    { id: 'gov',  label: 'Constraints' },
  ];
  return (
    <nav className="page-tabs">
      {tabs.map(tb => (
        <button key={tb.id} className={'page-tab' + (page === tb.id ? ' active' : '')} onClick={() => setPage(tb.id)}>
          {tb.label}
          {tb.count != null && <span className="tab-count">{tb.count}</span>}
        </button>
      ))}
      <div className="page-tabs-spacer"/>
      <span className="session-pill"><span className="live-dot"/>recording</span>
    </nav>
  );
}

// ====== Sidebar ======
function Sidebar({ activeNode, setActiveNode }) {
  const { constraints, sources, nodes } = RIGOR_DATA;
  const claims = nodes.filter(n => n.type === 'claim');
  return (
    <aside className="sidebar">
      <div className="side-section">
        <div className="side-eyebrow">Constraints<span className="count">{constraints.filter(c=>!c.retired).length} live</span></div>
        <ul className="side-list">
          {constraints.map(c => (
            <li key={c.id} className={'side-item' + (c.retired ? ' retired' : '')}>
              <div className="side-row">
                <span className={'live-dot-mini' + (c.live ? ' on' : '')}/>
                <span className="side-label" title={c.label}>{c.label}</span>
                <span className="t-eyebrow" style={{fontSize:9}}>{c.id}</span>
              </div>
              <div className="side-meta">{c.hits} hits · {c.blocks} blocks · {c.scope}</div>
            </li>
          ))}
        </ul>
      </div>

      <div className="side-section">
        <div className="side-eyebrow">Claims<span className="count">{claims.length}</span></div>
        <ul className="side-list">
          {claims.map(c => (
            <li key={c.id}
                className={'side-item' + (activeNode === c.id ? ' active' : '')}
                onClick={() => setActiveNode(c.id)}>
              <div className="side-row">
                <span className={'badge ' + (c.status==='pass'?'badge-support':c.status==='warn'?'badge-warn':'badge-violate')} style={{minWidth:18, justifyContent:'center'}}>{c.label}</span>
                <span className="side-label">{c.text}</span>
              </div>
            </li>
          ))}
        </ul>
      </div>

      <div className="side-section">
        <div className="side-eyebrow">Sources<span className="count">{sources.length}</span></div>
        <ul className="side-list">
          {sources.map(s => (
            <li key={s.id} className="side-source">
              <span className="src-kind">{s.kind}</span>
              <span className="src-label">{s.label}</span>
              <span className="src-n">{s.n}</span>
            </li>
          ))}
        </ul>
      </div>
    </aside>
  );
}

// ====== Stat strip ======
function Sparkline({ data, color }) {
  const w = 80, h = 24;
  const max = Math.max(...data, 1);
  const pts = data.map((v, i) => `${(i/(data.length-1))*w},${h - (v/max)*(h-2) - 1}`).join(' ');
  return (
    <svg className="spark" viewBox={`0 0 ${w} ${h}`} preserveAspectRatio="none">
      <polyline fill="none" stroke={color} strokeWidth="1.4" points={pts}/>
    </svg>
  );
}

function StatStrip({ totals, history }) {
  return (
    <div className="stat-strip">
      <div className="stat spark">
        <div className="stat-eyebrow">tokens / s</div>
        <div className="stat-value">{totals.tps}</div>
        <Sparkline data={history.tps} color="#1A1916"/>
      </div>
      <div className="stat pass">
        <div className="stat-eyebrow">grounded</div>
        <div className="stat-value">{totals.pass}</div>
        <div className="stat-meta">{totals.passRate}% of claims</div>
      </div>
      <div className="stat warn">
        <div className="stat-eyebrow">weak</div>
        <div className="stat-value">{totals.warn}</div>
        <div className="stat-meta">flagged for review</div>
      </div>
      <div className="stat block">
        <div className="stat-eyebrow">blocked</div>
        <div className="stat-value">{totals.block}</div>
        <div className="stat-meta">{totals.retracts} retracts</div>
      </div>
      <div className="stat">
        <div className="stat-eyebrow">judge latency</div>
        <div className="stat-value">{totals.judgeMs}<span style={{fontSize:13,color:'var(--ink-3)',marginLeft:4}}>ms</span></div>
        <div className="stat-meta">p95 · sliding 60s</div>
      </div>
    </div>
  );
}

window.RigorBaseComponents = { Topbar, PageTabs, Sidebar, StatStrip, Sparkline, I };

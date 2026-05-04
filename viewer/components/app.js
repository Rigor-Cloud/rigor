/* global React, ReactDOM, RigorBaseComponents, RigorGraphLog, RigorDrawer, RigorPages, useTweaks, TweaksPanel, TweakSection, TweakRadio, TweakToggle, TweakSelect */
const { useEffect, useMemo, useState, useRef } = React;
const { Topbar, PageTabs, Sidebar, StatStrip } = RigorBaseComponents;
const { ConstraintGraph, LogStrip } = RigorGraphLog;
const { Drawer } = RigorDrawer;
const { ObservabilityPage, SearchPage, EvalPage, ConstraintsPage } = RigorPages;

const TWEAK_DEFAULTS = /*EDITMODE-BEGIN*/{
  "layout": "3col",
  "density": "comfortable",
  "showSidebar": true,
  "graphPulse": true,
  "logFilter": "all"
}/*EDITMODE-END*/;

function AppInner() {
  const rigor = useRigorData();
  const [page, setPage] = useState('live');
  const [activeNode, setActiveNode] = useState(rigor.nodes.length ? rigor.nodes[0].id : null);
  const [activeEventId, setActiveEventId] = useState(rigor.events.length ? rigor.events[0].id : null);
  const [activeRequest, setActiveRequest] = useState(null);
  const [tweaks, setTweak] = useTweaks(TWEAK_DEFAULTS);

  // Listen for select-node from log strip
  useEffect(() => {
    const fn = (e) => setActiveNode(e.detail);
    window.addEventListener('select-node', fn);
    return () => window.removeEventListener('select-node', fn);
  }, []);

  // Listen for select-request from log strip — flips drawer to Stream tab.
  useEffect(() => {
    const fn = (e) => setActiveRequest(e.detail);
    window.addEventListener('select-request', fn);
    return () => window.removeEventListener('select-request', fn);
  }, []);

  // Derived counts — all from real data, no fakes
  const totals = useMemo(() => {
    const evs = rigor.events;
    const claims = evs.filter(e => e.kind === 'claim');
    const pass = claims.filter(c => c.status === 'pass').length;
    const warn = claims.filter(c => c.status === 'warn').length;
    const block = claims.filter(c => c.status === 'block').length;
    const retracts = evs.filter(e => e.kind === 'retract').length;
    return {
      tps: rigor.stats.tokens,
      pass, warn, block, retracts,
      passRate: Math.round((pass / Math.max(1, claims.length)) * 100),
      judgeMs: 0,
    };
  }, [rigor.events, rigor.stats]);

  const proxy = {
    tput: rigor.stats.requests,
    p95: 0,
    blocks: totals.block,
    connected: rigor.connected,
  };

  const history = useMemo(() => ({
    tps: [],
  }), []);

  const showLevels = useMemo(() => {
    if (tweaks.logFilter === 'all') return null;
    if (tweaks.logFilter === 'blocks') return { block: true };
    if (tweaks.logFilter === 'warn+block') return { block: true, warn: true };
    return null;
  }, [tweaks.logFilter]);

  return (
    <div className={'app' + (tweaks.density === 'dense' ? ' dense' : '')}>
      <Topbar proxy={proxy}/>
      <PageTabs page={page} setPage={setPage} blocks={totals.block}/>

      {page === 'live' && (
        <div className={'live-page' + (tweaks.layout === '2col' || !tweaks.showSidebar ? ' layout-2col' : '')}>
          {tweaks.showSidebar && tweaks.layout !== '2col' && (
            <Sidebar activeNode={activeNode} setActiveNode={setActiveNode}/>
          )}
          <div className="center">
            <ActionGateBanner/>
            <TimelineStrip/>
            <StatStrip totals={totals} history={history}/>
            <ConstraintGraph activeNode={activeNode} setActiveNode={setActiveNode} animPulse={tweaks.graphPulse}/>
            <JudgeLogStrip/>
            <LogStrip activeEventId={activeEventId} setActiveEventId={setActiveEventId} showLevels={showLevels}/>
          </div>
          <Drawer activeNode={activeNode} setActiveNode={setActiveNode} activeRequest={activeRequest}/>
        </div>
      )}

      {page === 'obs' && <ObservabilityPage/>}
      {page === 'search' && <SearchPage/>}
      {page === 'eval' && <EvalPage/>}
      {page === 'gov' && <ConstraintsPage/>}

      <TweaksPanel title="Tweaks">
        <TweakSection title="Layout">
          <TweakRadio label="Columns"
            options={[{label:'3 col', value:'3col'},{label:'2 col', value:'2col'}]}
            value={tweaks.layout} onChange={v=>setTweak('layout', v)}/>
          <TweakRadio label="Density"
            options={[{label:'Comfortable', value:'comfortable'},{label:'Dense', value:'dense'}]}
            value={tweaks.density} onChange={v=>setTweak('density', v)}/>
          <TweakToggle label="Show sidebar" value={tweaks.showSidebar} onChange={v=>setTweak('showSidebar', v)}/>
        </TweakSection>
        <TweakSection title="Live behaviour">
          <TweakToggle label="Graph pulse on active claim" value={tweaks.graphPulse} onChange={v=>setTweak('graphPulse', v)}/>
          <TweakSelect label="Log filter"
            options={[{label:'All events', value:'all'},{label:'Warn + block', value:'warn+block'},{label:'Blocks only', value:'blocks'}]}
            value={tweaks.logFilter} onChange={v=>setTweak('logFilter', v)}/>
        </TweakSection>
      </TweaksPanel>
    </div>
  );
}

// ====== Action gate banner — surfaces pending gates for approve/reject ======
function ActionGateBanner() {
  const rigor = useRigorData();
  const pending = (rigor.actionGates || []).filter(g => g.status === 'pending');
  if (pending.length === 0) return null;
  const decide = (gate_id, approve) => {
    fetch('/api/gate/' + encodeURIComponent(gate_id) + '/' + (approve ? 'approve' : 'reject'), { method: 'POST' }).catch(() => {});
  };
  return (
    <div className="action-gate-banner">
      {pending.map(g => (
        <div key={g.gate_id} className="action-gate-row">
          <span className="badge badge-warn">action gate</span>
          <span className="action-gate-text">{g.action_text}</span>
          {g.reason && <span className="action-gate-reason">{g.reason}</span>}
          <span style={{flex:1}}/>
          <button className="btn btn-secondary btn-sm" onClick={() => decide(g.gate_id, false)}>Reject</button>
          <button className="btn btn-primary btn-sm" onClick={() => decide(g.gate_id, true)}>Approve</button>
        </div>
      ))}
    </div>
  );
}

// ====== Timeline strip — per-request latency bars + retry/block markers ======
function TimelineStrip() {
  const rigor = useRigorData();
  const items = (rigor.timeline || []).slice(-80);
  if (items.length === 0) return null;
  const maxLat = Math.max(120, ...items.filter(i => i.latency != null).map(i => i.latency));
  return (
    <div className="timeline-strip">
      <span className="t-eyebrow" style={{paddingRight:8}}>timeline</span>
      <div className="timeline-track">
        {items.map((it, i) => {
          if (it.type === 'latency') {
            const h = Math.max(2, Math.round((it.latency / maxLat) * 18));
            return <span key={i} className="tl-bar" style={{height:h}} title={`${it.latency}ms`}/>;
          }
          if (it.type === 'block') return <span key={i} className="tl-mark tl-mark-block" title="block"/>;
          if (it.type === 'retry') return <span key={i} className="tl-mark tl-mark-retry" title="retry"/>;
          return null;
        })}
      </div>
    </div>
  );
}

// ====== Judge log strip — relevance + judge entries audit lane ======
function JudgeLogStrip() {
  const rigor = useRigorData();
  const entries = (rigor.judgeEntries || []).slice(-40);
  if (entries.length === 0) return null;
  return (
    <div className="judge-strip">
      <div className="judge-head">
        <span className="t-eyebrow">judge.log</span>
        <span style={{flex:1}}/>
        <span className="t-eyebrow" style={{color:'var(--ink-3)'}}>{entries.length}</span>
      </div>
      <div className="judge-body">
        {entries.slice().reverse().map(e => (
          <div key={e.id} className="judge-row">
            <span className="judge-kind">{e.kind}</span>
            <span className="judge-text">{e.text}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

function App() {
  return (
    <RigorDataProvider>
      <AppInner/>
    </RigorDataProvider>
  );
}

ReactDOM.createRoot(document.getElementById('root')).render(<App/>);

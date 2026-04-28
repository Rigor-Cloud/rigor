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

function App() {
  const [page, setPage] = useState('live');
  const [activeNode, setActiveNode] = useState('D');
  const [activeEventId, setActiveEventId] = useState('e5');
  const [tweaks, setTweak] = useTweaks(TWEAK_DEFAULTS);

  // Live throughput counters (animated)
  const [tick, setTick] = useState(0);
  useEffect(() => {
    const id = setInterval(() => setTick(t => (t+1) % 1000), 1200);
    return () => clearInterval(id);
  }, []);

  // Listen for select-node from log strip
  useEffect(() => {
    const fn = (e) => setActiveNode(e.detail);
    window.addEventListener('select-node', fn);
    return () => window.removeEventListener('select-node', fn);
  }, []);

  // Derived counts
  const totals = useMemo(() => {
    const evs = RIGOR_DATA.events;
    const claims = evs.filter(e => e.kind === 'claim');
    const pass = claims.filter(c => c.status === 'pass').length;
    const warn = claims.filter(c => c.status === 'warn').length;
    const block = claims.filter(c => c.status === 'block').length;
    const retracts = evs.filter(e => e.kind === 'retract').length;
    return {
      tps: 38 + Math.round(Math.sin(tick/2) * 6 + 4),
      pass, warn, block, retracts,
      passRate: Math.round((pass / Math.max(1, claims.length)) * 100),
      judgeMs: 124 + Math.round(Math.sin(tick/3) * 18),
    };
  }, [tick]);

  const proxy = {
    tput: 38 + Math.round(Math.sin(tick/2) * 6 + 4),
    p95: 312 + Math.round(Math.cos(tick/3) * 24),
    blocks: totals.block,
  };

  const history = useMemo(() => ({
    tps: Array.from({length: 24}, (_, i) => 30 + Math.round(Math.sin((tick+i)/2.5)*8 + Math.cos((tick+i)/4)*5 + 6)),
  }), [tick]);

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
            <StatStrip totals={totals} history={history}/>
            <ConstraintGraph activeNode={activeNode} setActiveNode={setActiveNode} animPulse={tweaks.graphPulse}/>
            <LogStrip activeEventId={activeEventId} setActiveEventId={setActiveEventId} showLevels={showLevels}/>
          </div>
          <Drawer activeNode={activeNode} setActiveNode={setActiveNode}/>
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

ReactDOM.createRoot(document.getElementById('root')).render(<App/>);

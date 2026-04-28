/* global React */
const { useState } = React;

// ====== Right Drawer ======
function Drawer({ activeNode, setActiveNode }) {
  const [tab, setTab] = useState('claim');
  const node = RIGOR_DATA.nodes.find(n => n.id === activeNode);
  const isClaim = node && node.type === 'claim';

  const incoming = RIGOR_DATA.edges.filter(e => e.to === activeNode);

  // Strength = sum of weighted edges, clamped
  const strength = (() => {
    if (!isClaim) return 0;
    const s = incoming.reduce((acc, e) => acc + e.w, 0);
    return Math.max(-1, Math.min(1, s));
  })();
  const pin = `calc(${((strength + 1) / 2) * 100}% - 1px)`;

  return (
    <aside className="drawer">
      <div className="drawer-tabs">
        {['claim','stream','governance'].map(t => (
          <button key={t} className={'drawer-tab' + (tab === t ? ' active' : '')} onClick={() => setTab(t)}>
            {t === 'claim' ? 'Inspector' : t === 'stream' ? 'Stream' : 'Policy'}
          </button>
        ))}
      </div>

      {tab === 'claim' && (
        isClaim ? <ClaimInspector node={node} incoming={incoming} strength={strength} pin={pin}/>
                : <DrawerEmpty/>
      )}
      {tab === 'stream' && <StreamView/>}
      {tab === 'governance' && <PolicyView/>}
    </aside>
  );
}

function DrawerEmpty() {
  return (
    <div className="drawer-empty"><div className="drawer-empty-inner">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5"><circle cx="12" cy="12" r="9"/><path d="M12 8v5l3 2"/></svg>
      <div className="t-body-sm">Select a claim in the graph or sidebar to inspect its support, attacks, and constraint hits.</div>
    </div></div>
  );
}

function ClaimInspector({ node, incoming, strength, pin }) {
  const supports = incoming.filter(e => e.kind === 'support');
  const attacks  = incoming.filter(e => e.kind === 'attack');
  const verdict =
    strength >= 0.5 ? { label: 'grounded', cls: 'passed' } :
    strength >= 0   ? { label: 'weak',     cls: 'warn' } :
                      { label: 'attacked', cls: 'blocked' };

  // Find judge event for this node
  const ev = RIGOR_DATA.events.find(e => e.target === node.id && e.kind === 'claim');

  return (
    <div className="drawer-content">
      <div className="drawer-head">
        <span className="drawer-id">claim {node.id}</span>
        <span className={'badge ' + (node.status==='pass'?'badge-support':node.status==='warn'?'badge-warn':'badge-violate')}>
          {node.status === 'pass' ? 'grounded' : node.status === 'warn' ? 'weak' : 'blocked'}
        </span>
      </div>

      <p className="drawer-claim">{node.text}</p>

      <div className="drawer-meta">
        <span>extracted 14:08:33</span>
        <span>·</span>
        <span>haiku-4-5</span>
        <span>·</span>
        <span>judge 124ms</span>
      </div>

      <div className="section">
        <div className="t-eyebrow">DF-QuAD strength</div>
        <div className="gauge-row">
          <span className="t-body-sm" style={{color:'var(--ink-3)'}}>{verdict.label}</span>
          <span className={'gauge-num ' + verdict.cls}>{strength >= 0 ? '+' : ''}{strength.toFixed(2)}</span>
        </div>
        <div className="gauge">
          <div className="gauge-track"/>
          <div className="gauge-mid"/>
          <div className="gauge-pin" style={{left: pin}}/>
        </div>
        <div className="gauge-axis">
          <span>−1.00 attack</span><span>0</span><span>+1.00 support</span>
        </div>
      </div>

      {ev && ev.reason && (
        <div className="section">
          <div className="t-eyebrow">judge note</div>
          <div className="t-body-sm" style={{lineHeight:1.5, color: 'var(--ink-2)'}}>{ev.reason}</div>
        </div>
      )}

      {supports.length > 0 && (
        <div className="section">
          <div className="t-eyebrow">support · {supports.length}</div>
          <ul className="edge-list">
            {supports.map((e, i) => <EdgeRow key={i} e={e}/>)}
          </ul>
        </div>
      )}

      {attacks.length > 0 && (
        <div className="section">
          <div className="t-eyebrow">attacks · {attacks.length}</div>
          <ul className="edge-list">
            {attacks.map((e, i) => <EdgeRow key={i} e={e}/>)}
          </ul>
        </div>
      )}

      <div className="drawer-actions">
        <button className="btn btn-secondary btn-sm">Open trace</button>
        <button className="btn btn-secondary btn-sm">Cite source</button>
        <button className="btn btn-ghost btn-sm">Mark resolved</button>
        {node.status === 'block' && <button className="btn btn-danger btn-sm" style={{marginLeft:'auto'}}>Force retract</button>}
      </div>
    </div>
  );
}

function EdgeRow({ e }) {
  const src = RIGOR_DATA.nodes.find(n => n.id === e.from);
  const cls = e.kind === 'support' ? 'support' : 'attack';
  return (
    <li className="edge">
      <span className={'edge-mark ' + cls}>{e.kind === 'support' ? '+' : '−'}</span>
      <div>
        <div className="edge-src">{src?.label || e.from} <span style={{color:'var(--ink-3)'}}>→ {e.to}</span></div>
        {e.excerpt && <div className="edge-excerpt">“{e.excerpt}”</div>}
      </div>
      <span className="edge-weight">{e.w >= 0 ? '+' : ''}{e.w.toFixed(2)}</span>
    </li>
  );
}

function StreamView() {
  return (
    <div className="drawer-content">
      <div className="t-eyebrow">model output · annotated</div>
      <div className="stream-block">
        {RIGOR_DATA.stream.map((s, i) => {
          if (s.cls === 'block') {
            return <span key={i} className="viol-mark" title="Blocked: forward-looking projection">{s.text}</span>;
          }
          if (s.cls === 'warn') {
            return <span key={i} style={{borderBottom: '1.5px dashed #B87A1A', cursor:'help'}} title="Weak grounding">{s.text}</span>;
          }
          if (s.cite) {
            return <span key={i}>{s.text}<sup style={{color:'var(--signal-support)', fontFamily:'var(--font-mono)', fontSize:9, marginLeft:1}}>[{s.cite}]</sup></span>;
          }
          return <span key={i}>{s.text}</span>;
        })}
      </div>
      <div className="t-eyebrow">retract preview</div>
      <div className="stream-block" style={{fontSize:13, color:'var(--ink-2)'}}>
        Acme’s Q3 was strong: revenue grew 12% year-over-year to $4.2B and operating margin reached 18%. <span style={{color:'var(--ink-3)'}}>It was the strongest quarter on record</span> — though prior periods were higher. No buybacks were announced.
      </div>
      <div className="drawer-actions">
        <button className="btn btn-secondary btn-sm">Apply retract</button>
        <button className="btn btn-ghost btn-sm">Diff source</button>
      </div>
    </div>
  );
}

function PolicyView() {
  const [flags, setFlags] = useState({
    enforce: true, judge: true, retract: true, redact: false, verbose: false,
  });
  const toggle = (k) => setFlags(f => ({...f, [k]: !f[k]}));
  const rows = [
    { k: 'enforce', label: 'Enforce constraints',     sub: 'block on violation' },
    { k: 'judge',   label: 'Live judge',              sub: 'haiku-4-5 · per-claim verdict' },
    { k: 'retract', label: 'Auto-retract',            sub: 'rewrite blocked spans' },
    { k: 'redact',  label: 'PII redaction',           sub: 'C-1133 · email/phone' },
    { k: 'verbose', label: 'Verbose audit log',       sub: 'include token-level deltas' },
  ];
  return (
    <div className="drawer-content">
      <div className="t-eyebrow">runtime policy</div>
      <div>
        {rows.map(r => (
          <div key={r.k} className="gov-row">
            <div>
              <div>{r.label}</div>
              <div className="gov-sub">{r.sub}</div>
            </div>
            <span className={'toggle' + (flags[r.k] ? ' on' : '')} onClick={() => toggle(r.k)}/>
          </div>
        ))}
      </div>
      <div className="t-eyebrow">danger zone</div>
      <div className="drawer-actions" style={{flexDirection:'column', alignItems:'stretch', gap:6, borderTop:0, marginTop:0, paddingTop:0}}>
        <button className="btn btn-secondary btn-sm">Pause judge</button>
        <button className="btn btn-secondary btn-sm">Replay session</button>
        <button className="btn btn-danger btn-sm">Drop session</button>
      </div>
    </div>
  );
}

window.RigorDrawer = { Drawer };

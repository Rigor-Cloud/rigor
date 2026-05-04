/* global React */
const { useState } = React;

// ====== Right Drawer ======
function Drawer({ activeNode, setActiveNode, activeRequest }) {
  const rigor = useRigorData();
  const [tab, setTab] = useState('claim');
  const node = rigor.nodes.find(n => n.id === activeNode);
  const isClaim = node && node.type === 'claim';

  const incoming = rigor.edges.filter(e => e.to === activeNode);

  // Strength = sum of weighted edges, clamped
  const strength = (() => {
    if (!isClaim) return 0;
    const s = incoming.reduce((acc, e) => acc + e.w, 0);
    return Math.max(-1, Math.min(1, s));
  })();
  const pin = `calc(${((strength + 1) / 2) * 100}% - 1px)`;

  // Auto-flip to Stream tab when a request gets selected externally.
  useEffect(() => {
    if (activeRequest) setTab('stream');
  }, [activeRequest]);

  return (
    <aside className="drawer">
      <div className="drawer-tabs">
        {['claim','stream','context','retries','governance'].map(t => (
          <button key={t} className={'drawer-tab' + (tab === t ? ' active' : '')} onClick={() => setTab(t)}>
            {t === 'claim' ? 'Inspector' : t === 'stream' ? 'Stream' : t === 'context' ? 'Context' : t === 'retries' ? 'Retries' : 'Policy'}
          </button>
        ))}
      </div>

      {tab === 'claim' && (
        isClaim ? <ClaimInspector node={node} incoming={incoming} strength={strength} pin={pin}/>
                : <DrawerEmpty/>
      )}
      {tab === 'stream' && <StreamView activeRequest={activeRequest}/>}
      {tab === 'context' && <ContextView/>}
      {tab === 'retries' && <RetriesView/>}
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
  const rigor = useRigorData();
  const supports = incoming.filter(e => e.kind === 'support');
  const attacks  = incoming.filter(e => e.kind === 'attack');
  const verdict =
    strength >= 0.5 ? { label: 'grounded', cls: 'passed' } :
    strength >= 0   ? { label: 'weak',     cls: 'warn' } :
                      { label: 'attacked', cls: 'blocked' };

  // Find judge event for this node
  const ev = rigor.events.find(e => e.target === node.id && e.kind === 'claim');

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
        <span>{node.epistemic ? `epistemic: ${node.epistemic}` : 'claim'}</span>
        {node.strength != null && <><span>·</span><span>strength {node.strength.toFixed(2)}</span></>}
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
  const rigor = useRigorData();
  const src = rigor.nodes.find(n => n.id === e.from);
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

function StreamView({ activeRequest }) {
  const rigor = useRigorData();
  // Pick the active request, else the most recently-touched streaming entry.
  const ids = Object.keys(rigor.streams || {});
  const fallbackId = ids.length ? ids[ids.length - 1] : null;
  const id = (activeRequest && rigor.streams[activeRequest]) ? activeRequest : fallbackId;
  const stream = id ? rigor.streams[id] : null;

  if (!stream) {
    return (
      <div className="drawer-empty"><div className="drawer-empty-inner">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5"><circle cx="12" cy="12" r="9"/><path d="M12 8v5l3 2"/></svg>
        <div className="t-body-sm">No active request. Click a request in the log strip to inspect its stream and any violation spans rigor flagged.</div>
      </div></div>
    );
  }

  // Build a list of text fragments interleaving violation-highlight spans for
  // each violation whose claim text we can locate inside the streamed output.
  const text = stream.text || '';
  const claims = rigor.nodes.filter(n => n.type === 'claim');
  const claimsById = Object.fromEntries(claims.map(c => [c.id, c]));
  const matches = [];
  (stream.violations || []).forEach(v => {
    const claim = claimsById[v.claim_id];
    const needle = claim?.text;
    if (!needle) return;
    const idx = text.indexOf(needle);
    if (idx >= 0) matches.push({ start: idx, end: idx + needle.length, v, claim });
  });
  matches.sort((a, b) => a.start - b.start);
  // Drop overlaps — keep the earliest.
  for (let i = 1; i < matches.length; i++) {
    if (matches[i].start < matches[i-1].end) { matches.splice(i, 1); i--; }
  }

  const fragments = [];
  let cursor = 0;
  matches.forEach((m, i) => {
    if (m.start > cursor) fragments.push(<span key={'t'+i}>{text.slice(cursor, m.start)}</span>);
    fragments.push(
      <span key={'v'+i} className="violation-highlight"
            title={(m.v.constraint_id || '') + ': ' + (m.v.reason || '')}>
        {text.slice(m.start, m.end)}
      </span>
    );
    cursor = m.end;
  });
  if (cursor < text.length) fragments.push(<span key="tail">{text.slice(cursor)}</span>);

  const statusBadgeCls =
    stream.status === 'blocked' ? 'badge-violate' :
    stream.status === 'allowed' ? 'badge-support' :
    stream.status === 'streaming' ? 'badge-warn' : 'badge-neutral';

  return (
    <div className="drawer-content">
      <div className="drawer-head">
        <span className="drawer-id">request {id.slice(0, 8)}</span>
        <span className={'badge ' + statusBadgeCls}>{stream.status}</span>
      </div>
      <div className="drawer-meta">
        <span>{stream.model || '—'}</span>
        {stream.durationMs != null && <><span>·</span><span>{stream.durationMs}ms</span></>}
        <span>·</span>
        <span>{(stream.violations || []).length} violation{(stream.violations || []).length === 1 ? '' : 's'}</span>
      </div>

      <div className="t-eyebrow">model output · annotated</div>
      <div className={'stream-block' + (stream.status === 'blocked' ? ' text-blocked' : '')}>
        {text ? fragments : <span style={{color:'var(--ink-3)'}}>(waiting for response…)</span>}
      </div>

      {stream.blockedText && (
        <>
          <div className="t-eyebrow">blocked text</div>
          <div className="stream-blocked-text">{stream.blockedText}</div>
        </>
      )}
      {stream.feedback && (
        <>
          <div className="t-eyebrow">retry feedback injected</div>
          <div className="stream-block" style={{fontSize:12, color:'var(--ink-2)'}}>{stream.feedback}</div>
        </>
      )}
    </div>
  );
}

function ContextView() {
  const rigor = useRigorData();
  const ctx = rigor.contextInjected;
  if (!ctx) {
    return (
      <div className="drawer-empty"><div className="drawer-empty-inner">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5"><rect x="4" y="4" width="16" height="16" rx="2"/><path d="M4 10h16"/></svg>
        <div className="t-body-sm">No context injection captured yet. The next request through the proxy will populate this view with the original system prompt and rigor's injected epistemic context.</div>
      </div></div>
    );
  }
  return (
    <div className="drawer-content">
      <div className="drawer-meta">
        <span>request {(ctx.request_id || '').slice(0, 8)}</span>
        {ctx.constraints_count != null && <><span>·</span><span>{ctx.constraints_count} constraints</span></>}
      </div>
      <div className="t-eyebrow">original system prompt</div>
      <pre className="ctx-pre">{ctx.original_system || '(none)'}</pre>
      <div className="t-eyebrow">rigor injected context</div>
      <pre className="ctx-pre">{ctx.context_preview || '(none)'}</pre>
    </div>
  );
}

function RetriesView() {
  const rigor = useRigorData();
  const retries = rigor.retries || [];
  if (retries.length === 0) {
    return (
      <div className="drawer-empty"><div className="drawer-empty-inner">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5"><path d="M3 12a9 9 0 1 0 3-6.7"/><path d="M3 4v5h5"/></svg>
        <div className="t-body-sm">No retries yet. When rigor blocks a response and triggers an auto-retry with violation feedback, those attempts surface here.</div>
      </div></div>
    );
  }
  return (
    <div className="drawer-content">
      {retries.slice().reverse().map((r, i) => (
        <div key={i} className="retry-card">
          <div className="drawer-head">
            <span className="drawer-id">request {(r.request_id || '').slice(0, 8)}</span>
            <span className={'badge ' + (r.status === 'retry_success' ? 'badge-support' : r.status === 'retry_failed' ? 'badge-violate' : 'badge-warn')}>{r.status}</span>
          </div>
          {r.blockedText && (
            <>
              <div className="t-eyebrow">blocked text</div>
              <div className="stream-blocked-text">{r.blockedText}</div>
            </>
          )}
          {r.feedback && (
            <>
              <div className="t-eyebrow">feedback injected</div>
              <div className="stream-block" style={{fontSize:12, color:'var(--ink-2)'}}>{r.feedback}</div>
            </>
          )}
          {r.retryResult && (
            <>
              <div className="t-eyebrow">retry result</div>
              <div className="stream-block" style={{fontSize:12}}>{r.retryResult}</div>
            </>
          )}
        </div>
      ))}
    </div>
  );
}

function PolicyView() {
  const rigor = useRigorData();
  const paused = !!rigor.governance.paused;
  const blockNext = !!rigor.governance.blockNext;

  const toggle = (path) => {
    fetch('/api/governance/' + path, { method: 'POST' }).catch(() => {});
  };
  const triggerRetry = () => {
    fetch('/api/governance/retry', { method: 'POST' }).catch(() => {});
  };

  return (
    <div className="drawer-content">
      <div className="t-eyebrow">runtime policy</div>
      <div>
        <div className="gov-row">
          <div>
            <div>Pause judge</div>
            <div className="gov-sub">stop evaluating claims; let traffic pass through</div>
          </div>
          <span className={'toggle' + (paused ? ' on' : '')} onClick={() => toggle('pause')}/>
        </div>
        <div className="gov-row">
          <div>
            <div>Block next response</div>
            <div className="gov-sub">force a block on the next decision (testing)</div>
          </div>
          <span className={'toggle' + (blockNext ? ' on' : '')} onClick={() => toggle('block-next')}/>
        </div>
      </div>
      <div className="t-eyebrow">manual</div>
      <div className="drawer-actions" style={{flexDirection:'column', alignItems:'stretch', gap:6, borderTop:0, marginTop:0, paddingTop:0}}>
        <button className="btn btn-secondary btn-sm" onClick={triggerRetry}>Retry last blocked</button>
      </div>
    </div>
  );
}

window.RigorDrawer = { Drawer };

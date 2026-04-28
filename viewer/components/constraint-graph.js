/* global React, RigorBaseComponents */
const { useEffect, useMemo, useRef, useState } = React;
const { I } = RigorBaseComponents;

// ====== Constraint Graph (SVG) ======
function ConstraintGraph({ activeNode, setActiveNode, animPulse }) {
  const { nodes, edges } = RIGOR_DATA;
  const W = 940, H = 600;

  const nodeMap = Object.fromEntries(nodes.map(n => [n.id, n]));

  // Edge color by kind + weight magnitude
  const edgeColor = (e) => {
    const a = Math.min(1, Math.abs(e.w));
    if (e.kind === 'support') return `rgba(46, 90, 122, ${0.35 + a*0.5})`;
    return `rgba(180, 58, 46, ${0.35 + a*0.5})`;
  };
  const edgeWidth = (e) => 1 + Math.abs(e.w) * 2.2;

  // Curved path between two points
  const path = (a, b) => {
    const mx = (a.x + b.x) / 2;
    const my = (a.y + b.y) / 2 - 20;
    return `M ${a.x} ${a.y} Q ${mx} ${my} ${b.x} ${b.y}`;
  };

  // Active claim's incoming edges, for highlight
  const incomingActive = new Set(
    edges.filter(e => e.to === activeNode).map(e => e.from + '→' + e.to)
  );

  return (
    <div className="graph-wrap" style={{minHeight:0}}>
      <div className="graph-grid-bg"/>
      <div className="graph-toolbar">
        <span className="legend-item"><span className="legend-dot support"/>support</span>
        <span className="legend-item"><span className="legend-dot attack"/>attack</span>
        <span className="legend-item"><span className="legend-dot claim"/>claim</span>
        <span className="legend-item"><span className="legend-dot blocked"/>blocked</span>
      </div>

      <svg className="graph-svg" viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="xMidYMid meet">
        <defs>
          <marker id="arrow-support" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto">
            <path d="M0,0 L10,5 L0,10 z" fill="#2E5A7A"/>
          </marker>
          <marker id="arrow-attack" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto">
            <path d="M0,0 L10,5 L0,10 z" fill="#B43A2E"/>
          </marker>
        </defs>

        {/* Edges */}
        {edges.map((e, i) => {
          const a = nodeMap[e.from], b = nodeMap[e.to];
          if (!a || !b) return null;
          const key = e.from + '→' + e.to;
          const active = incomingActive.has(key);
          return (
            <g key={i}>
              <path d={path(a,b)} fill="none"
                    stroke={edgeColor(e)}
                    strokeWidth={edgeWidth(e) + (active ? 1.4 : 0)}
                    strokeDasharray={e.kind==='attack' ? '6 4' : 'none'}
                    markerEnd={`url(#arrow-${e.kind})`}
                    opacity={active ? 1 : (activeNode ? 0.35 : 0.85)}/>
              <text x={(a.x+b.x)/2} y={(a.y+b.y)/2 - 24}
                    fontFamily="JetBrains Mono" fontSize="9"
                    fill={e.kind==='attack' ? '#8E2A20' : '#1F4360'}
                    opacity={active || !activeNode ? 0.85 : 0.25}
                    textAnchor="middle">
                {e.w > 0 ? '+' : ''}{e.w.toFixed(2)}
              </text>
            </g>
          );
        })}

        {/* Nodes */}
        {nodes.map((n) => {
          const isActive = activeNode === n.id;
          const pulse = animPulse && (n.id === 'G' || n.id === 'D');
          if (n.type === 'source') {
            return (
              <g key={n.id} transform={`translate(${n.x},${n.y})`} style={{cursor:'pointer'}}
                 onClick={() => setActiveNode(n.id)}>
                <rect x={-58} y={-18} width={116} height={36} rx={4} ry={4}
                      fill="#FAF7F0" stroke="#C9C2B0" strokeWidth="1"/>
                <text x={-50} y={-3} fontFamily="JetBrains Mono" fontSize="9" fill="#6E6A5E" letterSpacing="0.5">SOURCE</text>
                <text x={-50} y={11} fontFamily="Instrument Sans" fontSize="11" fill="#1A1916" fontWeight="600">{n.label}</text>
              </g>
            );
          }
          if (n.type === 'query') {
            return (
              <g key={n.id} transform={`translate(${n.x},${n.y})`}>
                <circle r={26} fill="#1A1916"/>
                <text y={4} textAnchor="middle" fontFamily="Instrument Sans" fontSize="11" fontWeight="600" fill="#FAF7F0">Q</text>
              </g>
            );
          }
          // Claim node
          const ringColor = n.status === 'pass' ? '#2E5A7A' : n.status === 'warn' ? '#B87A1A' : '#B43A2E';
          const fill = n.status === 'pass' ? '#D6E2EC' : n.status === 'warn' ? '#F4E6CB' : '#F5DDD7';
          return (
            <g key={n.id} transform={`translate(${n.x},${n.y})`} style={{cursor:'pointer'}}
               onClick={() => setActiveNode(n.id)}>
              {pulse && (
                <circle r="22" fill="none" stroke={ringColor} strokeWidth="1.2" opacity="0.7">
                  <animate attributeName="r" from="22" to="36" dur="1.8s" repeatCount="indefinite"/>
                  <animate attributeName="opacity" from="0.7" to="0" dur="1.8s" repeatCount="indefinite"/>
                </circle>
              )}
              <circle r={isActive ? 24 : 20} fill={fill} stroke={ringColor}
                      strokeWidth={isActive ? 2.2 : 1.4}/>
              <text textAnchor="middle" y="4" fontFamily="Instrument Sans" fontSize="13" fontWeight="700" fill={ringColor}>{n.label}</text>
              <text textAnchor="middle" y={36} fontFamily="Instrument Sans" fontSize="11" fill="#1A1916">
                {n.text.length > 28 ? n.text.slice(0, 27) + '…' : n.text}
              </text>
              {n.status === 'block' && (
                <g transform="translate(15,-15)">
                  <circle r="7" fill="#B43A2E"/>
                  <text textAnchor="middle" y="3" fontSize="9" fontWeight="700" fill="#fff" fontFamily="Instrument Sans">!</text>
                </g>
              )}
            </g>
          );
        })}
      </svg>

      <FloatingEvents />
    </div>
  );
}

// ====== Floating event cards ======
function FloatingEvents() {
  const [shown, setShown] = useState(RIGOR_DATA.events.slice(-3));
  useEffect(() => { setShown(RIGOR_DATA.events.slice(-3)); }, []);
  return (
    <div className="graph-events">
      {shown.slice().reverse().map(ev => {
        const cls = ev.status === 'block' ? 'violate' : ev.status === 'warn' ? 'warn' : ev.status === 'pass' ? 'support' : '';
        const badgeCls = ev.status === 'block' ? 'badge-violate' : ev.status === 'warn' ? 'badge-warn' : ev.status === 'pass' ? 'badge-support' : 'badge-neutral';
        const badgeText = ev.status === 'block' ? 'block' : ev.status === 'warn' ? 'warn' : ev.status === 'pass' ? 'grounded' : ev.status;
        return (
          <div key={ev.id} className={'event-card ' + cls}>
            <div className="evt-head">
              <span className={'badge ' + badgeCls}>{badgeText}</span>
              {ev.target && <span className="t-eyebrow" style={{fontSize:10}}>claim {ev.target}</span>}
              <span className="evt-time">{relTime(ev.t)}</span>
            </div>
            <div className="evt-body">{ev.text}</div>
            {ev.reason && (
              <div className="evt-meta">
                {ev.reason.split(':').slice(0,1).map((c,i) => <span key={i} className="evt-tag">{c.trim()}</span>)}
                <span style={{flex:1, color:'var(--ink-2)', fontFamily:'var(--font-ui)', fontSize:11}}>{ev.reason.split(':').slice(1).join(':').trim()}</span>
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}

// ====== Log strip (terminal) ======
function LogStrip({ activeEventId, setActiveEventId, showLevels }) {
  const events = useMemo(() => {
    const evs = RIGOR_DATA.events;
    if (!showLevels) return evs;
    return evs.filter(e => showLevels[e.status] !== false);
  }, [showLevels]);
  const bodyRef = useRef(null);
  useEffect(() => { if (bodyRef.current) bodyRef.current.scrollTop = bodyRef.current.scrollHeight; }, [events.length]);

  const lvlClass = (s) => 'log-lvl-' + (s === 'block' ? 'block' : s === 'warn' ? 'warn' : s === 'pass' ? 'pass' : s === 'retract' ? 'retract' : s === 'info' ? 'info' : 'claim');
  const lvlText = (s) => s === 'block' ? 'BLOCK' : s === 'warn' ? 'WARN' : s === 'pass' ? 'PASS' : s === 'retract' ? 'RETRACT' : s === 'info' ? 'INFO' : 'CLAIM';

  return (
    <div className="log-strip">
      <div className="log-head">
        <span className="log-title">judge.log</span>
        <span className="log-meta-r">tail -f · {events.length} events · session {RIGOR_DATA.sessionId}</span>
        <div className="log-spacer"/>
        <button className="log-icon-btn" title="copy">{I.copy}</button>
        <button className="log-icon-btn" title="download">{I.download}</button>
        <button className="log-icon-btn" title="more">{I.more}</button>
      </div>
      <div className="log-body" ref={bodyRef}>
        {events.map(ev => (
          <div key={ev.id}
               className={'log-row' + (activeEventId === ev.id ? ' active' : '')}
               onClick={() => { setActiveEventId(ev.id); if (ev.target) window.dispatchEvent(new CustomEvent('select-node', {detail: ev.target})); }}>
            <span className="log-ts">{fmtTime(ev.t)}</span>
            <span className={'log-lvl ' + lvlClass(ev.status)}>{lvlText(ev.status)}</span>
            <span className="log-msg">
              {ev.target ? <span style={{color:'var(--slab-fg-2)'}}>[{ev.target}] </span> : null}
              {ev.text}
              {ev.reason && <span style={{color:'var(--slab-fg-3)'}}> — {ev.reason}</span>}
            </span>
            <span className="log-meta-row">{ev.kind}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

window.RigorGraphLog = { ConstraintGraph, LogStrip };

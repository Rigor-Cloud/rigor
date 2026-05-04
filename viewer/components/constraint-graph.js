/* global React, RigorBaseComponents */
const { useEffect, useMemo, useRef, useState, useCallback } = React;
const { I } = RigorBaseComponents;

// ── Extensible style dictionaries ──
const NODE_COLORS = {
  constraint: { fill: '#E8E2D6', stroke: '#8A8272', text: '#1A1916' },
  claim:      { fill: '#F5DDD7', stroke: '#B43A2E', text: '#8E2A20' },
  source:     { fill: '#D6E2EC', stroke: '#2E5A7A', text: '#1F4360' },
  query:      { fill: '#1A1916', stroke: '#1A1916', text: '#FAF7F0' },
  justification: { fill: '#D6ECD8', stroke: '#3A7A2E', text: '#2A5A20' },
};
const EDGE_STYLES = {
  support:   { color: '#2E5A7A', dash: null },
  attack:    { color: '#B43A2E', dash: '5,3' },
  undercut:  { color: '#B87A1A', dash: '3,3' },
  violates:  { color: '#B43A2E', dash: '5,3' },
  justified_by: { color: '#3A7A2E', dash: null },
  derives_from: { color: '#6E6A5E', dash: '2,2' },
  contradicts:  { color: '#B43A2E', dash: '1,2' },
  anchors_at:   { color: '#8A8272', dash: '8,4' },
  semantically_similar_to: { color: '#7A5A8A', dash: '4,4' },
};
const EPISTEMIC_COLORS = {
  belief:        '#C97A4A',
  justification: '#4A8A5A',
  defeater:      '#5A6A8A',
};
const STATUS_RING = {
  pass: '#2E5A7A', warn: '#B87A1A', block: '#B43A2E',
};

// ── Inline force simulation (no d3 dependency) ──
function forceLayout(nodes, edges, W, H) {
  const N = nodes.length;
  if (N === 0) return nodes;

  // Initialize positions in a circle
  nodes.forEach((n, i) => {
    const angle = (i / N) * Math.PI * 2;
    const r = Math.min(W, H) * 0.35;
    n._x = W / 2 + Math.cos(angle) * r;
    n._y = H / 2 + Math.sin(angle) * r;
    n._vx = 0; n._vy = 0;
  });

  const idxMap = Object.fromEntries(nodes.map((n, i) => [n.id, i]));
  const edgeIdxs = edges.map(e => [idxMap[e.from], idxMap[e.to]]).filter(([a, b]) => a != null && b != null);

  const iterations = 200;
  for (let iter = 0; iter < iterations; iter++) {
    const alpha = 0.3 * (1 - iter / iterations);
    const repulse = 4000;

    // Repulsion (Barnes-Hut simplified — all pairs for <300 nodes is fine)
    for (let i = 0; i < N; i++) {
      for (let j = i + 1; j < N; j++) {
        let dx = nodes[j]._x - nodes[i]._x;
        let dy = nodes[j]._y - nodes[i]._y;
        let d2 = dx * dx + dy * dy + 1;
        let f = repulse / d2;
        nodes[i]._vx -= dx * f * alpha;
        nodes[i]._vy -= dy * f * alpha;
        nodes[j]._vx += dx * f * alpha;
        nodes[j]._vy += dy * f * alpha;
      }
    }

    // Attraction along edges
    const spring = 0.05;
    const idealLen = 80;
    for (const [ai, bi] of edgeIdxs) {
      let dx = nodes[bi]._x - nodes[ai]._x;
      let dy = nodes[bi]._y - nodes[ai]._y;
      let d = Math.sqrt(dx * dx + dy * dy) || 1;
      let f = (d - idealLen) * spring * alpha;
      let fx = (dx / d) * f;
      let fy = (dy / d) * f;
      nodes[ai]._vx += fx;
      nodes[ai]._vy += fy;
      nodes[bi]._vx -= fx;
      nodes[bi]._vy -= fy;
    }

    // Group clustering — same group nodes attract gently
    const groupCenters = {};
    nodes.forEach(n => {
      const g = n.group || 'default';
      if (!groupCenters[g]) groupCenters[g] = { sx: 0, sy: 0, c: 0 };
      groupCenters[g].sx += n._x;
      groupCenters[g].sy += n._y;
      groupCenters[g].c++;
    });
    for (const g in groupCenters) {
      groupCenters[g].sx /= groupCenters[g].c;
      groupCenters[g].sy /= groupCenters[g].c;
    }
    nodes.forEach(n => {
      const gc = groupCenters[n.group || 'default'];
      n._vx += (gc.sx - n._x) * 0.008 * alpha;
      n._vy += (gc.sy - n._y) * 0.008 * alpha;
    });

    // Centering
    nodes.forEach(n => {
      n._vx += (W / 2 - n._x) * 0.001 * alpha;
      n._vy += (H / 2 - n._y) * 0.001 * alpha;
    });

    // Apply velocity with damping
    nodes.forEach(n => {
      n._vx *= 0.6;
      n._vy *= 0.6;
      n._x += n._vx;
      n._y += n._vy;
      // Boundary clamp
      n._x = Math.max(60, Math.min(W - 60, n._x));
      n._y = Math.max(40, Math.min(H - 40, n._y));
    });
  }

  return nodes;
}

// ── Build domain groups from nodes ──
function buildGroups(nodes) {
  const groups = {};
  nodes.forEach(n => {
    const g = n.group || 'other';
    if (!groups[g]) groups[g] = { id: g, label: g, nodes: [], count: 0 };
    groups[g].nodes.push(n);
    groups[g].count++;
  });
  return Object.values(groups);
}

// ====== Constraint Graph (SVG, multi-level) ======
function ConstraintGraph({ activeNode, setActiveNode, animPulse }) {
  const rigor = useRigorData();
  const { nodes: rawNodes, edges } = rigor;
  const svgRef = useRef(null);
  const [zoom, setZoom] = useState({ x: 0, y: 0, k: 1 });
  const [expandedGroup, setExpandedGroup] = useState(null);
  const [dragging, setDragging] = useState(null);
  const W = 940, H = 600;

  // Assign groups from domain/scope
  const nodesWithGroups = useMemo(() =>
    rawNodes.map(n => ({ ...n, group: n.nodeType === 'constraint' ? (n.epistemic || 'other') : 'claims' })),
    [rawNodes]
  );

  const groups = useMemo(() => buildGroups(nodesWithGroups), [nodesWithGroups]);

  // Force layout for visible nodes (run once, memoized)
  const visibleNodes = useMemo(() => {
    let subset;
    if (expandedGroup) {
      const groupNodeIds = new Set(groups.find(g => g.id === expandedGroup)?.nodes.map(n => n.id) || []);
      // Show group nodes + their connected neighbors
      const neighborIds = new Set();
      edges.forEach(e => {
        if (groupNodeIds.has(e.from)) neighborIds.add(e.to);
        if (groupNodeIds.has(e.to)) neighborIds.add(e.from);
      });
      subset = nodesWithGroups.filter(n => groupNodeIds.has(n.id) || neighborIds.has(n.id));
    } else {
      // Show only constraint nodes (not claims) for overview
      subset = nodesWithGroups.filter(n => n.nodeType === 'constraint');
    }
    return forceLayout([...subset.map(n => ({...n}))], edges, W, H);
  }, [nodesWithGroups, edges, expandedGroup]);

  const visibleEdges = useMemo(() => {
    const nodeIds = new Set(visibleNodes.map(n => n.id));
    return edges.filter(e => nodeIds.has(e.from) && nodeIds.has(e.to));
  }, [visibleNodes, edges]);

  const nodeMap = useMemo(() =>
    Object.fromEntries(visibleNodes.map(n => [n.id, n])),
    [visibleNodes]
  );

  // Incoming edges for active node highlight
  const incomingActive = useMemo(() =>
    new Set(edges.filter(e => e.to === activeNode || e.from === activeNode).map(e => e.from + '→' + e.to)),
    [edges, activeNode]
  );

  // Edge rendering helpers
  const edgeStyle = (e) => EDGE_STYLES[e.kind] || EDGE_STYLES.support;
  const edgePath = (a, b) => {
    const dx = b._x - a._x, dy = b._y - a._y;
    const d = Math.sqrt(dx*dx + dy*dy) || 1;
    const cx = (a._x + b._x) / 2 - dy * 0.15;
    const cy = (a._y + b._y) / 2 + dx * 0.15;
    return `M ${a._x} ${a._y} Q ${cx} ${cy} ${b._x} ${b._y}`;
  };

  // Node radius by type + connections
  const nodeRadius = (n) => {
    const connections = edges.filter(e => e.from === n.id || e.to === n.id).length;
    const base = n.nodeType === 'constraint' ? 18 : 12;
    return base + Math.min(connections * 1.5, 12);
  };

  // Zoom/pan handlers
  const handleWheel = useCallback((e) => {
    e.preventDefault();
    const factor = e.deltaY > 0 ? 0.9 : 1.1;
    setZoom(z => ({ ...z, k: Math.max(0.3, Math.min(3, z.k * factor)) }));
  }, []);

  const handleMouseDown = useCallback((e) => {
    if (e.target.closest('.graph-node')) return;
    setDragging({ startX: e.clientX - zoom.x, startY: e.clientY - zoom.y });
  }, [zoom]);

  const handleMouseMove = useCallback((e) => {
    if (!dragging) return;
    setZoom(z => ({ ...z, x: e.clientX - dragging.startX, y: e.clientY - dragging.startY }));
  }, [dragging]);

  const handleMouseUp = useCallback(() => setDragging(null), []);

  // Breadcrumb for navigation
  const breadcrumb = expandedGroup
    ? `${groups.length} groups › ${expandedGroup} (${visibleNodes.length} nodes)`
    : `${visibleNodes.length} constraints · ${groups.length} groups · click a cluster to expand`;

  return (
    <div className="graph-wrap" style={{minHeight:0}}>
      <div className="graph-grid-bg"/>
      <div className="graph-toolbar">
        {expandedGroup && (
          <button className="graph-back-btn" onClick={() => { setExpandedGroup(null); setZoom({x:0,y:0,k:1}); }}
                  style={{background:'none',border:'1px solid #C9C2B0',borderRadius:4,padding:'2px 8px',cursor:'pointer',fontFamily:'inherit',fontSize:11,color:'#6E6A5E',marginRight:8}}>
            ← all groups
          </button>
        )}
        <span style={{fontSize:11,color:'#6E6A5E'}}>{breadcrumb}</span>
        <span style={{flex:1}}/>
        <span className="legend-item"><span className="legend-dot support"/>support</span>
        <span className="legend-item"><span className="legend-dot attack"/>attack</span>
        {Object.entries(EPISTEMIC_COLORS).map(([k, c]) => (
          <span key={k} className="legend-item"><span className="legend-dot" style={{background:c}}/>{k}</span>
        ))}
      </div>

      <svg className="graph-svg" viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="xMidYMid meet"
           ref={svgRef} onWheel={handleWheel}
           onMouseDown={handleMouseDown} onMouseMove={handleMouseMove} onMouseUp={handleMouseUp}
           style={{cursor: dragging ? 'grabbing' : 'grab'}}>
        <defs>
          <marker id="arrow-support" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
            <path d="M0,0 L10,5 L0,10 z" fill="#2E5A7A"/>
          </marker>
          <marker id="arrow-attack" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
            <path d="M0,0 L10,5 L0,10 z" fill="#B43A2E"/>
          </marker>
        </defs>

        <g transform={`translate(${zoom.x},${zoom.y}) scale(${zoom.k})`}>
          {/* Group background hulls (when not expanded) */}
          {!expandedGroup && groups.map(g => {
            const gNodes = visibleNodes.filter(n => n.group === g.id);
            if (gNodes.length === 0) return null;
            const cx = gNodes.reduce((s, n) => s + n._x, 0) / gNodes.length;
            const cy = gNodes.reduce((s, n) => s + n._y, 0) / gNodes.length;
            const maxDist = Math.max(40, ...gNodes.map(n => Math.sqrt((n._x-cx)**2 + (n._y-cy)**2)));
            const r = maxDist + 30;
            const color = EPISTEMIC_COLORS[g.id] || '#8A8272';
            return (
              <g key={'hull-'+g.id} style={{cursor:'pointer'}} onClick={() => { setExpandedGroup(g.id); setZoom({x:0,y:0,k:1}); }}>
                <circle cx={cx} cy={cy} r={r} fill={color} fillOpacity={0.06} stroke={color} strokeOpacity={0.2} strokeWidth={1} strokeDasharray="4,3"/>
                <text x={cx} y={cy - r + 14} textAnchor="middle" fontFamily="Instrument Sans" fontSize="11" fontWeight="600" fill={color} opacity={0.7}>
                  {g.label} ({g.count})
                </text>
              </g>
            );
          })}

          {/* Edges */}
          {visibleEdges.map((e, i) => {
            const a = nodeMap[e.from], b = nodeMap[e.to];
            if (!a || !b) return null;
            const key = e.from + '→' + e.to;
            const active = incomingActive.has(key);
            const style = edgeStyle(e);
            return (
              <path key={i} d={edgePath(a, b)} fill="none"
                    stroke={style.color}
                    strokeWidth={active ? 2.4 : 1.2}
                    strokeDasharray={style.dash || 'none'}
                    markerEnd={`url(#arrow-${e.kind === 'attack' || e.kind === 'violates' ? 'attack' : 'support'})`}
                    opacity={active ? 1 : (activeNode ? 0.2 : 0.5)}/>
            );
          })}

          {/* Nodes */}
          {visibleNodes.map(n => {
            const isActive = activeNode === n.id;
            const r = nodeRadius(n);
            const eColor = EPISTEMIC_COLORS[n.epistemic] || '#8A8272';
            const statusColor = STATUS_RING[n.status] || eColor;
            const nodeStyle = NODE_COLORS[n.nodeType] || NODE_COLORS.constraint;
            // Prefer human-readable name when distinct from id; for kebab IDs,
            // truncate at a word/hyphen boundary so labels don't snap mid-token.
            const fullLabel = n.label && n.label !== n.id ? n.label : (n.label || n.id);
            const labelMax = r < 16 ? 14 : 22;
            let label = fullLabel;
            if (label.length > labelMax) {
              const cut = label.lastIndexOf('-', labelMax);
              label = (cut > labelMax / 2 ? label.slice(0, cut) : label.slice(0, labelMax - 1)) + '…';
            }

            return (
              <g key={n.id} className="graph-node" transform={`translate(${n._x},${n._y})`}
                 style={{cursor:'pointer'}} onClick={() => setActiveNode(n.id)}>
                <title>{fullLabel}{n.text ? ` — ${n.text}` : ''}</title>
                {/* Epistemic type outer ring */}
                <circle r={r + 3} fill="none" stroke={eColor} strokeWidth={isActive ? 2.5 : 1} strokeOpacity={isActive ? 0.8 : 0.3}/>
                {/* Main circle */}
                <circle r={r} fill={nodeStyle.fill} stroke={statusColor} strokeWidth={isActive ? 2 : 1.2}/>
                {/* Strength indicator (arc) */}
                {n.strength != null && (
                  <circle r={r - 3} fill="none" stroke={statusColor} strokeWidth={2} strokeOpacity={0.3}
                          strokeDasharray={`${n.strength * (r-3) * Math.PI * 2} 999`}
                          transform="rotate(-90)"/>
                )}
                {/* Label below the node so the circle doesn't crop long IDs */}
                <text textAnchor="middle" y={r + 12} fontFamily="Instrument Sans" fontSize={r < 16 ? 9 : 10}
                      fontWeight="600" fill={nodeStyle.text}>
                  {label}
                </text>
                {/* Status badge */}
                {n.status === 'block' && (
                  <g transform={`translate(${r*0.7},-${r*0.7})`}>
                    <circle r="6" fill="#B43A2E"/>
                    <text textAnchor="middle" y="3" fontSize="8" fontWeight="700" fill="#fff" fontFamily="Instrument Sans">!</text>
                  </g>
                )}
                {n.status === 'warn' && (
                  <g transform={`translate(${r*0.7},-${r*0.7})`}>
                    <circle r="5" fill="#B87A1A"/>
                    <text textAnchor="middle" y="3" fontSize="7" fontWeight="700" fill="#fff" fontFamily="Instrument Sans">?</text>
                  </g>
                )}
              </g>
            );
          })}
        </g>
      </svg>

      <FloatingEvents />
    </div>
  );
}

// ====== Floating event cards ======
function FloatingEvents() {
  const rigor = useRigorData();
  const [shown, setShown] = useState(rigor.events.slice(-3));
  useEffect(() => { setShown(rigor.events.slice(-3)); }, [rigor.events]);
  return (
    <div className="graph-events">
      {shown.slice().reverse().map(ev => {
        const cls = ev.status === 'block' ? 'violate' : ev.status === 'warn' ? 'warn' : ev.status === 'pass' ? 'support' : '';
        const badgeCls = ev.status === 'block' ? 'badge-violate' : ev.status === 'warn' ? 'badge-warn' : ev.status === 'pass' ? 'badge-support' : 'badge-neutral';
        const badgeText = ev.status === 'block' ? 'block' : ev.status === 'warn' ? 'warn' : ev.status === 'pass' ? 'grounded' : ev.status;
        // Reason format from live-bridge.js is "<constraint-id>: <message>".
        // Split once so we can render the constraint id and the message
        // separately without duplicating the id in the title.
        const colonIdx = ev.reason ? ev.reason.indexOf(':') : -1;
        const reasonMsg = colonIdx >= 0 ? ev.reason.slice(colonIdx + 1).trim() : ev.reason;
        return (
          <div key={ev.id} className={'event-card ' + cls}>
            <div className="evt-head">
              <span className={'badge ' + badgeCls}>{badgeText}</span>
              {ev.target && <span className="evt-cst" title={ev.target}>{ev.target}</span>}
              <span className="evt-time">{relTime(ev.t)}</span>
            </div>
            <div className="evt-body">{ev.text}</div>
            {reasonMsg && (
              <div className="evt-reason">{reasonMsg}</div>
            )}
          </div>
        );
      })}
    </div>
  );
}

// ====== Log strip (terminal) ======
function LogStrip({ activeEventId, setActiveEventId, showLevels }) {
  const rigor = useRigorData();
  const events = useMemo(() => {
    const evs = rigor.events;
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
        <span className="log-meta-r">tail -f · {events.length} events · session {rigor.sessionId}</span>
        <div className="log-spacer"/>
        <button className="log-icon-btn" title="copy">{I.copy}</button>
        <button className="log-icon-btn" title="download">{I.download}</button>
        <button className="log-icon-btn" title="more">{I.more}</button>
      </div>
      <div className="log-body" ref={bodyRef}>
        {events.map(ev => (
          <div key={ev.id}
               className={'log-row' + (activeEventId === ev.id ? ' active' : '')}
               onClick={() => {
                 setActiveEventId(ev.id);
                 if (ev.kind === 'request' && ev.target) {
                   window.dispatchEvent(new CustomEvent('select-request', {detail: ev.target}));
                 } else if (ev.target) {
                   window.dispatchEvent(new CustomEvent('select-node', {detail: ev.target}));
                 }
               }}>
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

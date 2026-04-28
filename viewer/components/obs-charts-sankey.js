/* global React, RigorObsCharts1 */
const { useMemo, useState } = React;
const { ChartFrame, palette, rng, fmt, Legend } = RigorObsCharts1;

// ───────── 11. Source citation Sankey ─────────
function SourceSankey() {
  // 3 columns: sources → claim categories → verdict
  const data = useMemo(() => ({
    sources: [
      { id: 's1', label: 'Internal docs',   value: 380, color: palette.support[2] },
      { id: 's2', label: 'Web (allowed)',   value: 220, color: palette.support[1] },
      { id: 's3', label: 'User-supplied',   value: 130, color: '#8C7A5C' },
      { id: 's4', label: 'No source',       value: 170, color: palette.attack[1] },
    ],
    claims: [
      { id: 'c1', label: 'Numeric',     value: 280, color: palette.ink2 },
      { id: 'c2', label: 'Quote',       value: 180, color: palette.ink2 },
      { id: 'c3', label: 'Causal',      value: 220, color: palette.ink2 },
      { id: 'c4', label: 'Opinion',     value: 220, color: palette.ink3 },
    ],
    verdicts: [
      { id: 'v1', label: 'grounded', value: 600, color: palette.pass },
      { id: 'v2', label: 'weak',     value: 200, color: palette.warn },
      { id: 'v3', label: 'blocked',  value: 100, color: palette.block },
    ],
    flowsAB: [
      // source → claim
      ['s1','c1', 200], ['s1','c2', 80], ['s1','c3', 80], ['s1','c4', 20],
      ['s2','c1', 60],  ['s2','c2', 70], ['s2','c3', 50], ['s2','c4', 40],
      ['s3','c1', 18],  ['s3','c2', 22], ['s3','c3', 50], ['s3','c4', 40],
      ['s4','c1', 2],   ['s4','c2', 8],  ['s4','c3', 40], ['s4','c4', 120],
    ],
    flowsBC: [
      // claim → verdict
      ['c1','v1', 230], ['c1','v2', 40], ['c1','v3', 10],
      ['c2','v1', 140], ['c2','v2', 30], ['c2','v3', 10],
      ['c3','v1', 150], ['c3','v2', 50], ['c3','v3', 20],
      ['c4','v1', 80],  ['c4','v2', 80], ['c4','v3', 60],
    ],
  }), []);

  const W = 600, H = 320, pad = { l: 8, r: 8, t: 18, b: 10 };
  const colW = 100, gap = 8;
  const colsX = [pad.l + 100, (W - colW)/2, W - pad.r - colW - 100];
  const totalH = H - pad.t - pad.b;

  function layoutCol(items) {
    const total = items.reduce((s, it) => s + it.value, 0);
    let y = pad.t;
    return items.map(it => {
      const h = (it.value/total) * (totalH - (items.length-1)*gap);
      const r = { ...it, y, h };
      y += h + gap;
      return r;
    });
  }
  const colA = layoutCol(data.sources);
  const colB = layoutCol(data.claims);
  const colC = layoutCol(data.verdicts);
  const idx = arr => Object.fromEntries(arr.map(it => [it.id, it]));
  const A = idx(colA), B = idx(colB), C = idx(colC);

  // Position flows along node bands
  function buildFlows(flows, left, right, leftCol, rightCol) {
    // Track running offset within each node
    const leftOff = {}, rightOff = {};
    return flows.map(([a,b,v]) => {
      const lh = (v / leftCol.find(n=>n.id===a).value) * left[a].h;
      const rh = (v / rightCol.find(n=>n.id===b).value) * right[b].h;
      const ly = (left[a].y) + (leftOff[a] || 0);
      const ry = (right[b].y) + (rightOff[b] || 0);
      leftOff[a] = (leftOff[a] || 0) + lh;
      rightOff[b] = (rightOff[b] || 0) + rh;
      return { a, b, v, lh, rh, ly, ry };
    });
  }
  const flowsAB = buildFlows(data.flowsAB, A, B, colA, colB);
  const flowsBC = buildFlows(data.flowsBC, B, C, colB, colC);

  function flowPath(x1, y1, h1, x2, y2, h2) {
    const cx = (x1 + x2) / 2;
    return `M ${x1} ${y1} C ${cx} ${y1}, ${cx} ${y2}, ${x2} ${y2}
            L ${x2} ${y2+h2} C ${cx} ${y2+h2}, ${cx} ${y1+h1}, ${x1} ${y1+h1} Z`;
  }

  return (
    <ChartFrame title="Source → claim → verdict" sub="Sankey · last 24h · 900 claims" height={260}>
      <svg viewBox={`0 0 ${W} ${H}`} width="100%" height="100%" preserveAspectRatio="none">
        {/* Column headers */}
        <text x={colsX[0] + colW/2} y={12} fontSize="10" fontFamily="JetBrains Mono" fill={palette.ink3} textAnchor="middle">SOURCE</text>
        <text x={colsX[1] + colW/2} y={12} fontSize="10" fontFamily="JetBrains Mono" fill={palette.ink3} textAnchor="middle">CLAIM</text>
        <text x={colsX[2] + colW/2} y={12} fontSize="10" fontFamily="JetBrains Mono" fill={palette.ink3} textAnchor="middle">VERDICT</text>

        {/* Flows A→B */}
        {flowsAB.map((f, i) => (
          <path key={`ab-${i}`} d={flowPath(colsX[0]+colW, f.ly, f.lh, colsX[1], f.ry, f.rh)}
                fill={A[f.a].color} opacity="0.32"/>
        ))}
        {/* Flows B→C */}
        {flowsBC.map((f, i) => (
          <path key={`bc-${i}`} d={flowPath(colsX[1]+colW, f.ly, f.lh, colsX[2], f.ry, f.rh)}
                fill={C[f.b].color} opacity="0.32"/>
        ))}

        {/* Nodes */}
        {[colA, colB, colC].map((col, ci) => col.map(n => (
          <g key={n.id}>
            <rect x={colsX[ci]} y={n.y} width={colW} height={n.h} fill={n.color} opacity="0.9"/>
            <text x={ci === 0 ? colsX[0]-6 : colsX[ci]+colW+6}
                  y={n.y + n.h/2 - 2}
                  fontSize="11" fontFamily="var(--font-ui)" fontWeight="500" fill={palette.ink}
                  textAnchor={ci === 0 ? 'end' : 'start'}>{n.label}</text>
            <text x={ci === 0 ? colsX[0]-6 : colsX[ci]+colW+6}
                  y={n.y + n.h/2 + 11}
                  fontSize="9" fontFamily="JetBrains Mono" fill={palette.ink3}
                  textAnchor={ci === 0 ? 'end' : 'start'}>{n.value}</text>
          </g>
        )))}
      </svg>
    </ChartFrame>
  );
}

// ───────── 12. Coverage map (% claims supported) ─────────
function CoverageMap() {
  // Heat-strip per session/model
  const rows = useMemo(() => {
    const r = rng(131);
    const names = [
      'sess-7af2 · sonnet-4',
      'sess-7afa · sonnet-4',
      'sess-7afb · haiku-4',
      'sess-7b03 · gpt-4o',
      'sess-7b18 · sonnet-4',
      'sess-7b1c · gpt-4o-mini',
      'sess-7b22 · llama-3.3',
    ];
    return names.map((name, i) => {
      const seed = i;
      // 24 buckets across the session, each 0..1 coverage
      const bucks = Array.from({length: 24}, (_, k) => {
        const base = 0.55 + 0.3*Math.sin(k/4 + seed);
        return Math.max(0, Math.min(1, base + (r()-0.5)*0.2 - (i===5||i===6 ? 0.2 : 0)));
      });
      return { name, bucks, avg: bucks.reduce((a,b)=>a+b,0)/bucks.length };
    });
  }, []);
  const W = 600, H = 200, pad = { l: 130, r: 50, t: 6, b: 18 };
  const cellW = (W - pad.l - pad.r) / 24;
  const cellH = (H - pad.t - pad.b) / rows.length;
  const colorScale = (v) => {
    if (v < 0.5) return mix(palette.attack[1], palette.warn, v*2);
    return mix(palette.warn, palette.pass, (v-0.5)*2);
  };
  return (
    <ChartFrame title="Claim coverage" sub="% claims with ≥1 supporting edge · per session">
      <svg viewBox={`0 0 ${W} ${H}`} width="100%" height="100%" preserveAspectRatio="none">
        {rows.map((row, ri) => (
          <g key={ri}>
            <text x={pad.l - 8} y={pad.t + cellH*(ri+0.5) + 3} fontSize="10" fontFamily="JetBrains Mono"
                  fill={palette.ink2} textAnchor="end">{row.name}</text>
            {row.bucks.map((v, ci) => (
              <rect key={ci} x={pad.l + ci*cellW} y={pad.t + ri*cellH}
                    width={cellW-1} height={cellH-2} fill={colorScale(v)}/>
            ))}
            <text x={W-pad.r+6} y={pad.t + cellH*(ri+0.5) + 3} fontSize="10" fontFamily="JetBrains Mono"
                  fill={row.avg < 0.6 ? palette.block : palette.ink2}>{Math.round(row.avg*100)}%</text>
          </g>
        ))}
        {[0, 6, 12, 18, 23].map(h => (
          <text key={h} x={pad.l + cellW*(h+0.5)} y={H-4} textAnchor="middle"
                fontSize="9" fontFamily="JetBrains Mono" fill={palette.ink3}>t+{h}</text>
        ))}
      </svg>
    </ChartFrame>
  );
}

function mix(a, b, t) {
  const parse = c => c.startsWith('#') ?
    [parseInt(c.slice(1,3),16), parseInt(c.slice(3,5),16), parseInt(c.slice(5,7),16)] :
    c.match(/\d+/g).map(Number);
  const [ar,ag,ab] = parse(a), [br,bg,bb] = parse(b);
  return `rgb(${Math.round(ar+(br-ar)*t)},${Math.round(ag+(bg-ag)*t)},${Math.round(ab+(bb-ab)*t)})`;
}

// ───────── 13. Retract success funnel ─────────
function RetractFunnel() {
  const stages = [
    { label: 'Claims emitted',    value: 12420, color: palette.ink },
    { label: 'Claims blocked',    value:  1184, color: palette.block, drop: '90.5%' },
    { label: 'Retract sent',      value:  1184, color: palette.warn,  drop: '0.0%' },
    { label: 'Retract honored',   value:  1041, color: palette.support[2], drop: '12.1%' },
    { label: 'Accepted on retry', value:   862, color: palette.pass, drop: '17.2%' },
  ];
  const W = 600, H = 240, pad = { l: 20, r: 20, t: 14, b: 14 };
  const max = stages[0].value;
  const stepH = (H - pad.t - pad.b) / stages.length;
  return (
    <ChartFrame title="Retract & retry funnel" sub="claims emitted → blocked → retracted → accepted" height={200}>
      <svg viewBox={`0 0 ${W} ${H}`} width="100%" height="100%" preserveAspectRatio="none">
        {stages.map((s, i) => {
          const w = (s.value / max) * (W - pad.l - pad.r);
          const x0 = pad.l + ((W - pad.l - pad.r) - w) / 2;
          const y0 = pad.t + i * stepH;
          return (
            <g key={i}>
              <rect x={x0} y={y0+4} width={w} height={stepH-12} fill={s.color} opacity="0.85" rx="3"/>
              <text x={W/2} y={y0 + stepH/2 + 1}
                    fontSize="11" fontFamily="var(--font-ui)" fontWeight="600" fill="#fff" textAnchor="middle">
                {s.label} · {fmt(s.value)}
              </text>
              {s.drop && i > 0 && (
                <text x={W - pad.r} y={y0 + stepH/2 + 4}
                      fontSize="10" fontFamily="JetBrains Mono" fill={palette.ink3} textAnchor="end">
                  −{s.drop}
                </text>
              )}
            </g>
          );
        })}
      </svg>
    </ChartFrame>
  );
}

// ───────── 14. Session timeline strips ─────────
function SessionStrips() {
  const sessions = useMemo(() => {
    const r = rng(149);
    const names = [
      'sess-7c01', 'sess-7c08', 'sess-7c14', 'sess-7c22',
      'sess-7c33', 'sess-7c41', 'sess-7c52', 'sess-7c5e',
    ];
    return names.map((id, ix) => {
      const events = [];
      let t = r() * 5;
      while (t < 60) {
        const k = r();
        let kind = 'pass';
        if (k > 0.92) kind = 'block';
        else if (k > 0.78) kind = 'warn';
        events.push({ t, kind });
        t += r() * 4 + 0.5;
      }
      return { id, events, len: 60 };
    });
  }, []);
  const W = 600, H = 220, pad = { l: 80, r: 12, t: 8, b: 22 };
  const stripH = (H - pad.t - pad.b) / sessions.length - 4;
  const x = t => pad.l + (t/60) * (W - pad.l - pad.r);
  return (
    <ChartFrame title="Session timelines" sub="dots = events · color = verdict · 60s window" height={200}>
      <svg viewBox={`0 0 ${W} ${H}`} width="100%" height="100%" preserveAspectRatio="none">
        {sessions.map((s, ri) => {
          const yTop = pad.t + ri * (stripH + 4);
          return (
            <g key={ri}>
              <text x={pad.l-10} y={yTop + stripH/2 + 3} fontSize="10" fontFamily="JetBrains Mono" fill={palette.ink2} textAnchor="end">{s.id}</text>
              <rect x={pad.l} y={yTop} width={W-pad.r-pad.l} height={stripH} fill={palette.paper2} rx="2"/>
              {s.events.map((e, i) => {
                const c = e.kind === 'block' ? palette.block : e.kind === 'warn' ? palette.warn : palette.pass;
                const r = e.kind === 'block' ? 4 : e.kind === 'warn' ? 3 : 2;
                return <circle key={i} cx={x(e.t)} cy={yTop + stripH/2} r={r} fill={c}/>;
              })}
            </g>
          );
        })}
        {[0, 15, 30, 45, 60].map(t => (
          <text key={t} x={x(t)} y={H-6} fontSize="9" fontFamily="JetBrains Mono" fill={palette.ink3} textAnchor="middle">{t}s</text>
        ))}
      </svg>
    </ChartFrame>
  );
}

window.RigorObsCharts4 = { SourceSankey, CoverageMap, RetractFunnel, SessionStrips };

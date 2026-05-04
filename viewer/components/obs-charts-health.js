/* global React */
const { useMemo, useState } = React;

// ───────── Helpers ─────────
const palette = {
  pass: '#2E5A7A', warn: '#B87A1A', block: '#B43A2E',
  ink: '#1A1916', ink2: '#3D3A33', ink3: '#6E6A5E', ink4: '#98937F',
  paper: '#F4F1EA', paper2: '#ECE7DC', paper3: '#E2DCCC', bone: '#FAF7F0',
  rule: '#C9C2B0', rule2: '#D9D2BF',
  attack: ['#F5DDD7','#D88B82','#B43A2E','#8E2A20'],
  support: ['#D6E2EC','#7A9CB0','#2E5A7A','#1F4360'],
};
const fmt = (n, d=0) => n.toLocaleString(undefined, {maximumFractionDigits: d});

// Deterministic pseudorandom
function rng(seed) { let s = seed; return () => { s = (s*1664525+1013904223)|0; return ((s>>>0)%10000)/10000; }; }

// ───────── Frame chrome ─────────
function ChartFrame({ title, sub, right, children, height = 160 }) {
  return (
    <div style={{
      background: palette.bone, border: `1px solid ${palette.rule}`,
      borderRadius: 6, overflow: 'hidden', display: 'flex', flexDirection: 'column'
    }}>
      <div style={{
        display:'flex', alignItems:'baseline', gap: 10,
        padding: '8px 12px 6px', borderBottom: `1px solid ${palette.rule2}`,
        background: palette.paper2,
      }}>
        <div style={{fontFamily:'var(--font-ui)', fontSize:12, fontWeight:600, color:palette.ink}}>{title}</div>
        {sub && <div style={{fontFamily:'var(--font-mono)', fontSize:9.5, color:palette.ink3}}>{sub}</div>}
        <span style={{fontFamily:'var(--font-mono)', fontSize:9, fontWeight:600, letterSpacing:'0.06em', textTransform:'uppercase', color:palette.ink3, padding:'1px 6px', border:`1px solid ${palette.rule}`, borderRadius:3, background:palette.paper3}}>preview</span>
        <div style={{flex:1}}/>
        {right}
      </div>
      <div style={{flex:1, padding: 10, height}}>{children}</div>
    </div>
  );
}

// ───────── 1. Stacked area: grounded / weak / blocked over 24h ─────────
function StackedHealthChart() {
  const W = 600, H = 200, pad = { l: 36, r: 12, t: 8, b: 22 };
  const data = useMemo(() => {
    const r = rng(7);
    return Array.from({length: 24}, (_, h) => {
      const total = 100 + Math.round(r()*60 + Math.sin(h/3)*30);
      const block = Math.max(0, Math.round(total * (0.04 + r()*0.05 + (h>=14&&h<=16?0.04:0))));
      const warn  = Math.max(0, Math.round(total * (0.10 + r()*0.06)));
      const pass  = total - block - warn;
      return { h, pass, warn, block, total };
    });
  }, []);
  const maxY = Math.max(...data.map(d => d.total));
  const x = i => pad.l + (i/(data.length-1)) * (W - pad.l - pad.r);
  const y = v => H - pad.b - (v/maxY) * (H - pad.t - pad.b);
  const stack = (key1, key2) => data.map((d,i) => `${x(i)},${y((d[key1]||0)+(d[key2]||0))}`).join(' ');
  const passLine  = data.map((d,i) => `${x(i)},${y(d.pass)}`).join(' ');
  const passWarn  = stack('pass','warn');
  const allLine   = stack('pass','warn').replace; // unused — build manually
  const top       = data.map((d,i) => `${x(i)},${y(d.pass+d.warn+d.block)}`).join(' ');
  const baseline  = `${x(data.length-1)},${y(0)} ${x(0)},${y(0)}`;
  return (
    <ChartFrame
      title="Verdict mix over 24h"
      sub="grounded · weak · blocked"
      right={<Legend items={[
        {label:'grounded', color: palette.pass},
        {label:'weak', color: palette.warn},
        {label:'blocked', color: palette.block},
      ]}/>}
    >
      <svg viewBox={`0 0 ${W} ${H}`} width="100%" height="100%" preserveAspectRatio="none">
        {[0, 0.5, 1].map((f,i) => (
          <line key={i} x1={pad.l} x2={W-pad.r} y1={y(maxY*f)} y2={y(maxY*f)} stroke={palette.rule2}/>
        ))}
        {/* blocked (top) */}
        <polygon points={`${top} ${baseline}`} fill={palette.block} opacity="0.65"/>
        {/* warn band */}
        <polygon points={`${passWarn} ${baseline}`} fill={palette.warn} opacity="0.7"/>
        {/* pass band */}
        <polygon points={`${passLine} ${baseline}`} fill={palette.pass} opacity="0.85"/>
        {/* x ticks */}
        {[0, 6, 12, 18, 23].map(h => (
          <text key={h} x={x(h)} y={H-6} fontSize="9" fontFamily="JetBrains Mono" fill={palette.ink3} textAnchor="middle">
            {String(h).padStart(2,'0')}:00
          </text>
        ))}
        {[0, 0.5, 1].map((f,i) => (
          <text key={i} x={pad.l-6} y={y(maxY*f)+3} fontSize="9" fontFamily="JetBrains Mono" fill={palette.ink3} textAnchor="end">
            {Math.round(maxY*f)}
          </text>
        ))}
      </svg>
    </ChartFrame>
  );
}

// ───────── Legend ─────────
function Legend({ items }) {
  return (
    <div style={{display:'flex', gap:10}}>
      {items.map(it => (
        <span key={it.label} style={{display:'inline-flex', alignItems:'center', gap:5, fontFamily:'var(--font-mono)', fontSize:10, color:palette.ink3}}>
          <span style={{width:10, height:8, background:it.color, borderRadius:2}}/>{it.label}
        </span>
      ))}
    </div>
  );
}

// ───────── 2. Judge latency p50/p95/p99 ─────────
function LatencyChart() {
  const W = 600, H = 200, pad = { l: 36, r: 12, t: 8, b: 22 };
  const data = useMemo(() => {
    const r = rng(11);
    return Array.from({length: 60}, (_, i) => {
      const base = 90 + Math.sin(i/8)*15;
      return {
        p50: Math.round(base + r()*8),
        p95: Math.round(base*1.4 + r()*30 + (i>=38&&i<=44?80:0)),
        p99: Math.round(base*2.0 + r()*60 + (i>=38&&i<=44?160:0)),
      };
    });
  }, []);
  const budget = 250;
  const maxY = Math.max(420, ...data.map(d => d.p99));
  const x = i => pad.l + (i/(data.length-1)) * (W - pad.l - pad.r);
  const y = v => H - pad.b - (v/maxY) * (H - pad.t - pad.b);
  const line = key => data.map((d,i) => `${x(i)},${y(d[key])}`).join(' ');

  // budget breach band
  const breach = [];
  let inBreach = null;
  data.forEach((d,i) => {
    if (d.p95 > budget) {
      if (inBreach == null) inBreach = i;
    } else if (inBreach != null) { breach.push([inBreach, i-1]); inBreach = null; }
  });
  if (inBreach != null) breach.push([inBreach, data.length-1]);

  return (
    <ChartFrame
      title="Judge latency"
      sub="p50 / p95 / p99 · last 60 min"
      right={<Legend items={[
        {label:'p50', color: palette.support[1]},
        {label:'p95', color: palette.support[2]},
        {label:'p99', color: palette.attack[2]},
      ]}/>}
    >
      <svg viewBox={`0 0 ${W} ${H}`} width="100%" height="100%" preserveAspectRatio="none">
        {/* breach bands */}
        {breach.map(([s,e],i) => (
          <rect key={i} x={x(s)} y={pad.t} width={x(e)-x(s)} height={H-pad.t-pad.b}
                fill={palette.attack[0]} opacity="0.5"/>
        ))}
        {/* budget line */}
        <line x1={pad.l} x2={W-pad.r} y1={y(budget)} y2={y(budget)} stroke={palette.warn} strokeDasharray="4 3" strokeWidth="1"/>
        <text x={W-pad.r-4} y={y(budget)-3} fontSize="9" fontFamily="JetBrains Mono" fill={palette.warn} textAnchor="end">budget {budget}ms</text>
        {/* gridlines */}
        {[0, 0.5, 1].map((f,i) => (
          <line key={i} x1={pad.l} x2={W-pad.r} y1={y(maxY*f)} y2={y(maxY*f)} stroke={palette.rule2}/>
        ))}
        <polyline fill="none" stroke={palette.support[1]} strokeWidth="1.4" points={line('p50')}/>
        <polyline fill="none" stroke={palette.support[2]} strokeWidth="1.6" points={line('p95')}/>
        <polyline fill="none" stroke={palette.attack[2]} strokeWidth="1.6" points={line('p99')}/>
        {/* axis */}
        {[0, 0.5, 1].map((f,i) => (
          <text key={i} x={pad.l-6} y={y(maxY*f)+3} fontSize="9" fontFamily="JetBrains Mono" fill={palette.ink3} textAnchor="end">{Math.round(maxY*f)}ms</text>
        ))}
        {[0, 30, 59].map(t => (
          <text key={t} x={x(t)} y={H-6} fontSize="9" fontFamily="JetBrains Mono" fill={palette.ink3} textAnchor="middle">−{60-t}m</text>
        ))}
      </svg>
    </ChartFrame>
  );
}

// ───────── 3. Throughput with annotations ─────────
function ThroughputChart() {
  const W = 600, H = 180, pad = { l: 32, r: 12, t: 8, b: 22 };
  const data = useMemo(() => {
    const r = rng(3);
    return Array.from({length: 90}, (_, i) => {
      const drift = i > 60 ? 14 : 0;
      return Math.max(2, Math.round(28 + Math.sin(i/9)*8 + r()*6 + drift));
    });
  }, []);
  const annots = [
    { i: 18, label: 'C-1305 enabled', kind: 'policy' },
    { i: 42, label: 'deploy v0.8.2',  kind: 'deploy' },
    { i: 70, label: 'judge → haiku-4-5', kind: 'config' },
  ];
  const maxY = Math.max(...data) * 1.1;
  const x = i => pad.l + (i/(data.length-1)) * (W - pad.l - pad.r);
  const y = v => H - pad.b - (v/maxY) * (H - pad.t - pad.b);
  const path = data.map((v,i) => `${x(i)},${y(v)}`).join(' ');
  const area = `${x(0)},${y(0)} ${path} ${x(data.length-1)},${y(0)}`;
  return (
    <ChartFrame title="Request throughput" sub="req/s · 90m · annotated">
      <svg viewBox={`0 0 ${W} ${H}`} width="100%" height="100%" preserveAspectRatio="none">
        {[0, 0.5, 1].map((f,i) => (
          <line key={i} x1={pad.l} x2={W-pad.r} y1={y(maxY*f)} y2={y(maxY*f)} stroke={palette.rule2}/>
        ))}
        <polygon points={area} fill={palette.support[2]} opacity="0.12"/>
        <polyline fill="none" stroke={palette.support[2]} strokeWidth="1.6" points={path}/>
        {annots.map((a, idx) => (
          <g key={idx}>
            <line x1={x(a.i)} x2={x(a.i)} y1={pad.t} y2={H-pad.b} stroke={palette.ink} strokeDasharray="2 3" opacity="0.4"/>
            <rect x={x(a.i)+2} y={pad.t+idx*16} width={a.label.length*5.5+8} height={13} fill={palette.bone} stroke={palette.ink} rx="2"/>
            <text x={x(a.i)+6} y={pad.t+idx*16+9} fontSize="9" fontFamily="JetBrains Mono" fill={palette.ink}>{a.label}</text>
          </g>
        ))}
        {[0, 30, 60, 89].map(t => (
          <text key={t} x={x(t)} y={H-6} fontSize="9" fontFamily="JetBrains Mono" fill={palette.ink3} textAnchor="middle">−{90-t}m</text>
        ))}
        {[0, 0.5, 1].map((f,i) => (
          <text key={i} x={pad.l-6} y={y(maxY*f)+3} fontSize="9" fontFamily="JetBrains Mono" fill={palette.ink3} textAnchor="end">{Math.round(maxY*f)}</text>
        ))}
      </svg>
    </ChartFrame>
  );
}

// ───────── 4. Small multiples: block rate by model ─────────
function ModelSparkGrid() {
  const models = useMemo(() => {
    const r = rng(19);
    return [
      { name: 'claude-sonnet-4',   rate: 0.012, n: 4128, sigma: 0.4 },
      { name: 'claude-haiku-4',    rate: 0.018, n: 1822, sigma: 0.5 },
      { name: 'claude-opus-4',     rate: 0.008, n:  342, sigma: 0.3 },
      { name: 'gpt-4o',            rate: 0.027, n: 2410, sigma: 0.7 },
      { name: 'gpt-4o-mini',       rate: 0.034, n: 1090, sigma: 0.9 },
      { name: 'llama-3.3-70b',     rate: 0.046, n:  588, sigma: 1.1 },
    ].map(m => ({
      ...m,
      data: Array.from({length: 30}, (_, i) => Math.max(0, m.rate + Math.sin(i/4)*0.004*m.sigma + (r()-0.5)*0.006*m.sigma))
    }));
  }, []);
  const maxRate = Math.max(...models.flatMap(m => m.data));
  return (
    <ChartFrame title="Block rate by model" sub="last 24h · 30 buckets" height={200}>
      <div style={{display:'grid', gridTemplateColumns:'1fr 1fr', gap:10, height:'100%', overflow:'auto'}}>
        {models.map(m => {
          const W = 220, H = 38;
          const x = i => (i/(m.data.length-1)) * W;
          const y = v => H - 2 - (v/maxRate) * (H-4);
          const path = m.data.map((v,i) => `${x(i)},${y(v)}`).join(' ');
          const area = `0,${H} ${path} ${W},${H}`;
          const cls = m.rate >= 0.03 ? palette.block : m.rate >= 0.02 ? palette.warn : palette.pass;
          return (
            <div key={m.name} style={{display:'grid', gridTemplateColumns:'1fr auto', gap:6, alignItems:'center'}}>
              <div>
                <div style={{fontFamily:'var(--font-mono)', fontSize:11, color:palette.ink, fontWeight:500}}>{m.name}</div>
                <div style={{fontFamily:'var(--font-mono)', fontSize:9, color:palette.ink3}}>{fmt(m.n)} reqs · <span style={{color: cls, fontWeight:600}}>{(m.rate*100).toFixed(2)}%</span></div>
              </div>
              <svg width={W} height={H} viewBox={`0 0 ${W} ${H}`}>
                <polygon points={area} fill={cls} opacity="0.18"/>
                <polyline points={path} fill="none" stroke={cls} strokeWidth="1.4"/>
              </svg>
            </div>
          );
        })}
      </div>
    </ChartFrame>
  );
}

window.RigorObsCharts1 = { ChartFrame, Legend, palette, fmt, rng, StackedHealthChart, LatencyChart, ThroughputChart, ModelSparkGrid };

/* global React, RigorObsCharts1 */
const { useMemo } = React;
const { ChartFrame, Legend, palette, rng, fmt } = RigorObsCharts1;

// ───────── 5. Claims-per-response histogram ─────────
function ClaimsHistogram() {
  const W = 600, H = 200, pad = { l: 32, r: 12, t: 8, b: 26 };
  const data = useMemo(() => {
    // bucket by claim count 1..20
    const r = rng(31);
    const out = Array.from({length: 20}, (_, i) => 0);
    const total = 1200;
    for (let i = 0; i < total; i++) {
      // Mixture: gamma-ish around 3 + small bump near 12 (verbose models)
      const main = Math.max(1, Math.min(20, Math.round(2 + Math.pow(r(), 0.6) * 4)));
      const verbose = r() < 0.18 ? Math.max(7, Math.min(20, Math.round(9 + r()*5))) : main;
      out[verbose - 1]++;
    }
    return out;
  }, []);
  const maxY = Math.max(...data);
  const x = i => pad.l + (i+0.5) * (W - pad.l - pad.r) / data.length;
  const y = v => H - pad.b - (v/maxY) * (H - pad.t - pad.b);
  const bw = (W - pad.l - pad.r) / data.length - 2;
  const median = (() => {
    let cum = 0; const half = data.reduce((a,b)=>a+b,0)/2;
    for (let i = 0; i < data.length; i++) { cum += data[i]; if (cum >= half) return i+1; }
    return 1;
  })();
  return (
    <ChartFrame title="Claims per response" sub="distribution · 1.2k responses · 24h">
      <svg viewBox={`0 0 ${W} ${H}`} width="100%" height="100%" preserveAspectRatio="none">
        {[0, 0.5, 1].map((f,i) => (
          <line key={i} x1={pad.l} x2={W-pad.r} y1={y(maxY*f)} y2={y(maxY*f)} stroke={palette.rule2}/>
        ))}
        {data.map((v, i) => {
          const cx = x(i);
          return (
            <rect key={i} x={cx - bw/2} y={y(v)} width={bw} height={H-pad.b - y(v)}
                  fill={i+1 >= 8 ? palette.warn : palette.support[2]} opacity={i+1 === median ? 1 : 0.85}/>
          );
        })}
        <line x1={x(median-1)} x2={x(median-1)} y1={pad.t} y2={H-pad.b} stroke={palette.ink} strokeDasharray="3 3"/>
        <text x={x(median-1)+4} y={pad.t+10} fontSize="9" fontFamily="JetBrains Mono" fill={palette.ink}>median {median}</text>
        {[1, 5, 10, 15, 20].map(c => (
          <text key={c} x={x(c-1)} y={H-10} fontSize="9" fontFamily="JetBrains Mono" fill={palette.ink3} textAnchor="middle">{c}</text>
        ))}
        <text x={(W)/2} y={H-2} fontSize="9" fontFamily="JetBrains Mono" fill={palette.ink3} textAnchor="middle">claims per response</text>
        {[0, 0.5, 1].map((f,i) => (
          <text key={i} x={pad.l-6} y={y(maxY*f)+3} fontSize="9" fontFamily="JetBrains Mono" fill={palette.ink3} textAnchor="end">{Math.round(maxY*f)}</text>
        ))}
      </svg>
    </ChartFrame>
  );
}

// ───────── 6. Strength distribution (DF-QuAD score) ─────────
function StrengthDistribution() {
  const W = 600, H = 220, pad = { l: 32, r: 12, t: 8, b: 26 };
  const bins = 41; // -1 to +1, step 0.05
  const data = useMemo(() => {
    const r = rng(53);
    const out = new Array(bins).fill(0);
    const N = 2400;
    for (let i = 0; i < N; i++) {
      // Bimodal: cluster near +0.6 (grounded) and near -0.4 (attacked); slight zero hump
      const which = r();
      let v;
      if (which < 0.62) v = 0.55 + (r()-0.5)*0.45;
      else if (which < 0.84) v = -0.35 + (r()-0.5)*0.40;
      else v = (r()-0.5)*0.30;
      v = Math.max(-1, Math.min(1, v));
      const idx = Math.round((v+1)/2 * (bins-1));
      out[idx]++;
    }
    return out;
  }, []);
  const maxY = Math.max(...data);
  const x = i => pad.l + (i/(bins-1)) * (W - pad.l - pad.r);
  const y = v => H - pad.b - (v/maxY) * (H - pad.t - pad.b);
  const bw = (W - pad.l - pad.r) / bins - 1;
  const colorAt = i => {
    const v = -1 + 2*i/(bins-1);
    if (v < -0.5) return palette.attack[2];
    if (v < 0)    return palette.attack[1];
    if (v < 0.5)  return palette.support[1];
    return palette.support[2];
  };
  return (
    <ChartFrame
      title="DF-QuAD strength distribution"
      sub="−1 attack · 0 ungrounded · +1 supported"
      right={<Legend items={[
        {label:'attacked', color: palette.attack[2]},
        {label:'weak', color: palette.attack[1]},
        {label:'supported', color: palette.support[2]},
      ]}/>}
    >
      <svg viewBox={`0 0 ${W} ${H}`} width="100%" height="100%" preserveAspectRatio="none">
        {/* threshold bands */}
        <rect x={x(0)} y={pad.t} width={x(bins-1)-x(0)} height={H-pad.t-pad.b} fill={palette.support[0]} opacity="0.3"/>
        <rect x={pad.l} y={pad.t} width={x(0)-pad.l} height={H-pad.t-pad.b} fill={palette.attack[0]} opacity="0.3"/>
        {data.map((v, i) => (
          <rect key={i} x={x(i)-bw/2} y={y(v)} width={bw} height={H-pad.b-y(v)} fill={colorAt(i)}/>
        ))}
        {/* zero line */}
        <line x1={x((bins-1)/2)} x2={x((bins-1)/2)} y1={pad.t} y2={H-pad.b} stroke={palette.ink}/>
        {[-1, -0.5, 0, 0.5, 1].map((v, i) => {
          const idx = (v+1)/2 * (bins-1);
          return <text key={i} x={x(idx)} y={H-10} fontSize="9" fontFamily="JetBrains Mono" fill={palette.ink3} textAnchor="middle">{v >= 0 ? '+' : ''}{v.toFixed(1)}</text>;
        })}
        <text x={W/2} y={H-2} fontSize="9" fontFamily="JetBrains Mono" fill={palette.ink3} textAnchor="middle">strength</text>
        {[0, 0.5, 1].map((f,i) => (
          <text key={i} x={pad.l-6} y={y(maxY*f)+3} fontSize="9" fontFamily="JetBrains Mono" fill={palette.ink3} textAnchor="end">{Math.round(maxY*f)}</text>
        ))}
      </svg>
    </ChartFrame>
  );
}

// ───────── 7. Time-to-first-block (cumulative) ─────────
function TimeToFirstBlock() {
  const W = 600, H = 200, pad = { l: 36, r: 12, t: 8, b: 26 };
  const data = useMemo(() => {
    // CDF of seconds-to-first-block; concentrated 1-8s tail
    const r = rng(67);
    const N = 200;
    const samples = [];
    for (let i = 0; i < N; i++) {
      // Log-normal-ish
      const u1 = r() || 0.001;
      const u2 = r();
      const z = Math.sqrt(-2*Math.log(u1)) * Math.cos(2*Math.PI*u2);
      const v = Math.max(0.2, Math.exp(0.9 + z*0.55));
      samples.push(v);
    }
    samples.sort((a,b) => a-b);
    return samples;
  }, []);
  const maxX = 20; // seconds cap
  const x = v => pad.l + Math.min(1, v/maxX) * (W - pad.l - pad.r);
  const y = v => H - pad.b - v * (H - pad.t - pad.b);
  const N = data.length;
  const path = data.map((v, i) => `${x(v)},${y((i+1)/N)}`).join(' ');
  const median = data[Math.floor(N*0.5)];
  const p90 = data[Math.floor(N*0.9)];
  return (
    <ChartFrame title="Time to first block" sub="seconds from stream start · CDF">
      <svg viewBox={`0 0 ${W} ${H}`} width="100%" height="100%" preserveAspectRatio="none">
        {[0, 0.25, 0.5, 0.75, 1].map((f,i) => (
          <line key={i} x1={pad.l} x2={W-pad.r} y1={y(f)} y2={y(f)} stroke={palette.rule2}/>
        ))}
        <polyline fill="none" stroke={palette.block} strokeWidth="1.8" points={path}/>
        {/* P50 marker */}
        <line x1={x(median)} x2={x(median)} y1={pad.t} y2={y(0.5)} stroke={palette.ink} strokeDasharray="3 3"/>
        <line x1={pad.l} x2={x(median)} y1={y(0.5)} y2={y(0.5)} stroke={palette.ink} strokeDasharray="3 3"/>
        <circle cx={x(median)} cy={y(0.5)} r="3" fill={palette.ink}/>
        <text x={x(median)+6} y={y(0.5)-6} fontSize="9" fontFamily="JetBrains Mono" fill={palette.ink}>P50 · {median.toFixed(1)}s</text>
        <line x1={x(p90)} x2={x(p90)} y1={pad.t} y2={y(0.9)} stroke={palette.warn} strokeDasharray="3 3"/>
        <circle cx={x(p90)} cy={y(0.9)} r="3" fill={palette.warn}/>
        <text x={x(p90)+6} y={y(0.9)-6} fontSize="9" fontFamily="JetBrains Mono" fill={palette.warn}>P90 · {p90.toFixed(1)}s</text>
        {[0, 5, 10, 15, 20].map(s => (
          <text key={s} x={x(s)} y={H-10} fontSize="9" fontFamily="JetBrains Mono" fill={palette.ink3} textAnchor="middle">{s}s</text>
        ))}
        <text x={W/2} y={H-2} fontSize="9" fontFamily="JetBrains Mono" fill={palette.ink3} textAnchor="middle">time</text>
        {[0, 0.5, 1].map((f,i) => (
          <text key={i} x={pad.l-6} y={y(f)+3} fontSize="9" fontFamily="JetBrains Mono" fill={palette.ink3} textAnchor="end">{Math.round(f*100)}%</text>
        ))}
      </svg>
    </ChartFrame>
  );
}

window.RigorObsCharts2 = { ClaimsHistogram, StrengthDistribution, TimeToFirstBlock };

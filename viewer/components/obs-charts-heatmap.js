/* global React, RigorObsCharts1 */
const { useMemo, useState } = React;
const { ChartFrame, palette, rng, fmt } = RigorObsCharts1;

// ───────── 8. Constraint × hour heatmap ─────────
function ConstraintHeatmap() {
  const constraints = ['C-1041','C-2003','C-1305','C-1102','C-1133','C-1410','C-2110','C-1502'];
  const grid = useMemo(() => {
    const r = rng(73);
    return constraints.map((id, ci) => Array.from({length: 24}, (_, h) => {
      // Weekday rhythm + per-constraint baseline
      const base = [0.45, 0.32, 0.20, 0.25, 0.06, 0.04, 0.18, 0.10][ci];
      const dayBoost = Math.max(0, Math.sin((h-6)/8 * Math.PI)) * 0.7;
      const noise = r() * 0.4;
      // Spike for C-2003 around 14:00 (matches Live demo)
      const spike = (ci === 1 && h >= 13 && h <= 15) ? 0.8 : 0;
      return Math.max(0, Math.round((base + dayBoost*0.6 + noise + spike) * 30));
    }));
  }, []);
  const max = Math.max(...grid.flat(), 1);
  const W = 600, H = 220, pad = { l: 64, r: 12, t: 8, b: 22 };
  const cellW = (W - pad.l - pad.r) / 24;
  const cellH = (H - pad.t - pad.b) / constraints.length;
  const colorScale = (v) => {
    if (v === 0) return palette.paper2;
    const t = v / max;
    // Interpolate paper → warn → block
    if (t < 0.5) {
      const k = t * 2;
      return mix(palette.paper2, palette.warn, k * 0.7);
    } else {
      const k = (t - 0.5) * 2;
      return mix(palette.warn, palette.block, k);
    }
  };
  return (
    <ChartFrame title="Constraint hits · hour-of-day" sub="rows = constraint · cols = hour · 7-day average" height={210}>
      <svg viewBox={`0 0 ${W} ${H}`} width="100%" height="100%" preserveAspectRatio="none">
        {grid.map((row, ri) => (
          <g key={ri}>
            <text x={pad.l - 8} y={pad.t + cellH*(ri+0.5) + 3} textAnchor="end" fontSize="10" fontFamily="JetBrains Mono" fill={palette.ink2}>{constraints[ri]}</text>
            {row.map((v, ci) => (
              <g key={ci}>
                <rect x={pad.l + ci*cellW} y={pad.t + ri*cellH}
                      width={cellW-1} height={cellH-1}
                      fill={colorScale(v)} stroke={palette.bone} strokeWidth="0.5"/>
                {v >= max*0.55 && (
                  <text x={pad.l + ci*cellW + cellW/2} y={pad.t + ri*cellH + cellH/2 + 3}
                        textAnchor="middle" fontSize="9" fontFamily="JetBrains Mono" fill="#fff" fontWeight="600">{v}</text>
                )}
              </g>
            ))}
          </g>
        ))}
        {[0, 6, 12, 18, 23].map(h => (
          <text key={h} x={pad.l + cellW*(h+0.5)} y={H-6} textAnchor="middle"
                fontSize="9" fontFamily="JetBrains Mono" fill={palette.ink3}>{String(h).padStart(2,'0')}</text>
        ))}
        {/* Legend */}
        <g transform={`translate(${W-pad.r-110},${pad.t})`}>
          {Array.from({length: 10}, (_, i) => (
            <rect key={i} x={i*10} y={0} width={10} height={6} fill={colorScale((i/9)*max)}/>
          ))}
          <text x={0} y={18} fontSize="9" fontFamily="JetBrains Mono" fill={palette.ink3}>0</text>
          <text x={100} y={18} fontSize="9" fontFamily="JetBrains Mono" fill={palette.ink3} textAnchor="end">{max}</text>
        </g>
      </svg>
    </ChartFrame>
  );
}

// hex mix
function mix(a, b, t) {
  const pa = parseInt(a.slice(1), 16), pb = parseInt(b.slice(1), 16);
  const ar = (pa>>16)&255, ag=(pa>>8)&255, ab=pa&255;
  const br = (pb>>16)&255, bg=(pb>>8)&255, bb=pb&255;
  const r = Math.round(ar+(br-ar)*t), g = Math.round(ag+(bg-ag)*t), b2 = Math.round(ab+(bb-ab)*t);
  return `rgb(${r},${g},${b2})`;
}

// ───────── 9. Co-occurrence matrix ─────────
function CooccurrenceMatrix() {
  const ids = ['C-1041','C-2003','C-1305','C-1102','C-1133','C-1410','C-2110','C-1502'];
  // Symmetric strengths in [0..1]; engineered to highlight C-2003+C-1305 (speculative) and C-1041+C-1305
  const M = useMemo(() => {
    const r = rng(89);
    const m = ids.map(() => ids.map(() => 0));
    for (let i = 0; i < ids.length; i++) {
      for (let j = 0; j <= i; j++) {
        if (i === j) m[i][j] = 1;
        else m[i][j] = m[j][i] = +(r()*0.3).toFixed(2);
      }
    }
    // Engineered correlations
    const set = (a, b, v) => { const i = ids.indexOf(a), j = ids.indexOf(b); m[i][j] = m[j][i] = v; };
    set('C-2003','C-1305', 0.78);
    set('C-1041','C-1305', 0.51);
    set('C-1102','C-1305', 0.42);
    set('C-2003','C-1041', 0.36);
    set('C-1133','C-1410', 0.62);
    return m;
  }, []);
  const W = 600, H = 320, pad = { l: 64, r: 60, t: 56, b: 14 };
  const cell = Math.min((W - pad.l - pad.r) / ids.length, (H - pad.t - pad.b) / ids.length);
  const colorScale = (v) => v >= 0.99 ? palette.ink : mix(palette.paper, palette.support[2], v*1.05);
  return (
    <ChartFrame title="Constraint co-occurrence" sub="how often two constraints fire on the same response" height={260}>
      <svg viewBox={`0 0 ${W} ${H}`} width="100%" height="100%" preserveAspectRatio="xMinYMin meet">
        {/* Column headers (rotated) */}
        {ids.map((id, ci) => (
          <text key={ci} transform={`translate(${pad.l + ci*cell + cell/2}, ${pad.t - 6}) rotate(-40)`}
                fontSize="10" fontFamily="JetBrains Mono" fill={palette.ink2}>{id}</text>
        ))}
        {/* Row labels */}
        {ids.map((id, ri) => (
          <text key={ri} x={pad.l-8} y={pad.t + ri*cell + cell/2 + 3}
                fontSize="10" fontFamily="JetBrains Mono" fill={palette.ink2} textAnchor="end">{id}</text>
        ))}
        {/* Cells */}
        {M.map((row, ri) => row.map((v, ci) => {
          const isDiag = ri === ci;
          const isStrong = v >= 0.4;
          return (
            <g key={`${ri}-${ci}`}>
              <rect x={pad.l + ci*cell} y={pad.t + ri*cell}
                    width={cell-1} height={cell-1}
                    fill={isDiag ? palette.paper3 : colorScale(v)}
                    stroke={palette.bone} strokeWidth="0.5"/>
              {!isDiag && isStrong && (
                <text x={pad.l + ci*cell + cell/2} y={pad.t + ri*cell + cell/2 + 3}
                      textAnchor="middle" fontSize="9" fontFamily="JetBrains Mono"
                      fill={v > 0.6 ? '#fff' : palette.ink}>{v.toFixed(2)}</text>
              )}
            </g>
          );
        }))}
        {/* Color legend */}
        <g transform={`translate(${W-pad.r+8}, ${pad.t})`}>
          {Array.from({length: 12}, (_, i) => (
            <rect key={i} x={0} y={i*8} width={14} height={8} fill={colorScale(i/11)}/>
          ))}
          <text x={20} y={6} fontSize="9" fontFamily="JetBrains Mono" fill={palette.ink3}>1.0</text>
          <text x={20} y={11*8 + 6} fontSize="9" fontFamily="JetBrains Mono" fill={palette.ink3}>0.0</text>
        </g>
      </svg>
    </ChartFrame>
  );
}

// ───────── 10. Recall vs FP scatter ─────────
function RecallFpScatter() {
  const W = 600, H = 280, pad = { l: 44, r: 16, t: 14, b: 36 };
  const items = useMemo(() => {
    const r = rng(101);
    return [
      { id: 'C-1041', label: 'Numeric must cite',     hits: 184 },
      { id: 'C-2003', label: 'No forward-looking',    hits: 96  },
      { id: 'C-1305', label: 'Opinion vs fact',       hits: 71  },
      { id: 'C-1102', label: 'Quote attribution',     hits: 58  },
      { id: 'C-1133', label: 'PII redaction',         hits: 22  },
      { id: 'C-1410', label: 'No medical advice',     hits: 11  },
      { id: 'C-2110', label: 'Tense consistency',     hits: 28  },
      { id: 'C-1502', label: 'Causal claim cited',    hits: 44  },
      { id: 'C-1701', label: 'Salary disclaimers',    hits: 9   },
      { id: 'C-1820', label: 'Date drift check',      hits: 17  },
    ].map((it, i) => ({
      ...it,
      recall: Math.max(0.4, Math.min(0.99, 0.6 + r()*0.4 - (it.hits < 20 ? 0.15 : 0))),
      fp:     Math.max(0.01, Math.min(0.4, 0.05 + r()*0.18 + (it.hits < 20 ? 0.12 : 0))),
    }));
  }, []);
  const x = v => pad.l + v * (W - pad.l - pad.r);          // FP rate 0..0.4
  const y = v => H - pad.b - v * (H - pad.t - pad.b);      // recall 0..1
  const xv = v => v / 0.4;
  return (
    <ChartFrame title="Recall vs. false-positive" sub="bubble size = hit count · top-left = good" height={240}>
      <svg viewBox={`0 0 ${W} ${H}`} width="100%" height="100%" preserveAspectRatio="none">
        {/* Quadrant tint: top-left good (subtle) */}
        <rect x={pad.l} y={pad.t} width={x(xv(0.10))-pad.l} height={y(0.8)-pad.t} fill={palette.support[0]} opacity="0.35"/>
        <rect x={x(xv(0.20))} y={y(0.8)} width={W-pad.r - x(xv(0.20))} height={H-pad.b - y(0.8)} fill={palette.attack[0]} opacity="0.35"/>
        {/* Axes */}
        <line x1={pad.l} y1={H-pad.b} x2={W-pad.r} y2={H-pad.b} stroke={palette.rule}/>
        <line x1={pad.l} y1={pad.t} x2={pad.l} y2={H-pad.b} stroke={palette.rule}/>
        {/* Gridlines */}
        {[0.25, 0.5, 0.75, 1].map(f => (
          <line key={f} x1={pad.l} x2={W-pad.r} y1={y(f)} y2={y(f)} stroke={palette.rule2}/>
        ))}
        {/* Axis labels */}
        {[0, 0.1, 0.2, 0.3, 0.4].map(v => (
          <g key={v}>
            <text x={x(xv(v))} y={H-pad.b+14} fontSize="9" fontFamily="JetBrains Mono" fill={palette.ink3} textAnchor="middle">{(v*100).toFixed(0)}%</text>
          </g>
        ))}
        {[0.25, 0.5, 0.75, 1].map(v => (
          <text key={v} x={pad.l-6} y={y(v)+3} fontSize="9" fontFamily="JetBrains Mono" fill={palette.ink3} textAnchor="end">{(v*100).toFixed(0)}%</text>
        ))}
        <text x={(pad.l+W-pad.r)/2} y={H-6} fontSize="10" fontFamily="JetBrains Mono" fill={palette.ink3} textAnchor="middle">false-positive rate →</text>
        <text x={pad.l-30} y={(pad.t+H-pad.b)/2} fontSize="10" fontFamily="JetBrains Mono" fill={palette.ink3}
              transform={`rotate(-90 ${pad.l-30} ${(pad.t+H-pad.b)/2})`} textAnchor="middle">recall →</text>
        {/* Bubbles */}
        {items.map((it, i) => {
          const r = 4 + Math.sqrt(it.hits) * 0.7;
          const cls = it.fp > 0.20 ? palette.block : it.recall < 0.7 ? palette.warn : palette.pass;
          return (
            <g key={i}>
              <circle cx={x(xv(it.fp))} cy={y(it.recall)} r={r} fill={cls} opacity="0.55" stroke={cls} strokeWidth="1.4"/>
              <text x={x(xv(it.fp)) + r + 4} y={y(it.recall)+3}
                    fontSize="10" fontFamily="JetBrains Mono" fill={palette.ink}>{it.id}</text>
            </g>
          );
        })}
      </svg>
    </ChartFrame>
  );
}

window.RigorObsCharts3 = { ConstraintHeatmap, CooccurrenceMatrix, RecallFpScatter };

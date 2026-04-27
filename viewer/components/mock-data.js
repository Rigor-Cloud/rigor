// ========= Mock data + helpers =========
window.RIGOR_DATA = (function () {
  const now = Date.now();
  const t = (sAgo) => new Date(now - sAgo * 1000);

  const sources = [
    { id: 'sec-10k', kind: 'PDF',  label: 'Acme 10-K FY24',         n: 142 },
    { id: 'note-q3', kind: 'MD',   label: 'Q3 internal review',     n: 28 },
    { id: 'press',   kind: 'WEB',  label: 'press.acme/oct-2024',    n: 14 },
    { id: 'guide',   kind: 'POL',  label: 'Disclosure policy v3.1', n: 9 },
  ];

  // Constraint set
  const constraints = [
    { id: 'C-1041', label: 'Numeric claim must cite source',     scope: 'finance', live: true,  hits: 4, blocks: 2 },
    { id: 'C-2003', label: 'No forward-looking projections',     scope: 'finance', live: true,  hits: 1, blocks: 1 },
    { id: 'C-1102', label: 'Quote attribution required',         scope: 'editorial', live: true, hits: 2, blocks: 0 },
    { id: 'C-1133', label: 'PII redaction (email/phone)',        scope: 'safety', live: true,   hits: 0, blocks: 0 },
    { id: 'C-1305', label: 'Distinguish opinion vs fact',        scope: 'editorial', live: true, hits: 1, blocks: 0 },
    { id: 'C-1410', label: 'No medical advice',                  scope: 'safety', live: false,  hits: 0, blocks: 0, retired: true },
  ];

  // Claim graph nodes
  const nodes = [
    // Anchor (the prompt)
    { id: 'Q',    type: 'query',    x: 90,  y: 320, label: 'User query',     text: 'Summarize Acme’s Q3 results and outlook.' },
    // Sources
    { id: 'S1',   type: 'source',   x: 90,  y: 90,  label: '10-K FY24',      text: 'Annual report, p.42'  },
    { id: 'S2',   type: 'source',   x: 90,  y: 180, label: 'Q3 review',      text: 'Internal note 24/10/12' },
    { id: 'S3',   type: 'source',   x: 90,  y: 480, label: 'Press release',  text: 'press.acme, 24/10/30' },
    // Claims being made
    { id: 'A',    type: 'claim',    x: 360, y: 130, label: 'A',  text: 'Revenue grew 12% YoY in Q3.',            grounded: true,  status: 'pass'  },
    { id: 'B',    type: 'claim',    x: 360, y: 240, label: 'B',  text: 'Operating margin reached 18%.',          grounded: true,  status: 'pass'  },
    { id: 'C',    type: 'claim',    x: 600, y: 200, label: 'C',  text: 'The strongest quarter on record.',       grounded: false, status: 'warn'  },
    { id: 'D',    type: 'claim',    x: 360, y: 380, label: 'D',  text: 'Margins will expand further next year.', grounded: false, status: 'block' },
    { id: 'E',    type: 'claim',    x: 600, y: 380, label: 'E',  text: 'Driven by AI tooling rollout.',          grounded: false, status: 'warn'  },
    { id: 'F',    type: 'claim',    x: 360, y: 500, label: 'F',  text: 'No buybacks announced.',                  grounded: true,  status: 'pass'  },
    { id: 'G',    type: 'claim',    x: 820, y: 280, label: 'G',  text: 'Acme will outperform peers.',            grounded: false, status: 'block' },
  ];

  // Edges: support (+) or attack (-) with weight in [-1,1]
  const edges = [
    // Source → Claim grounding
    { from: 'S1', to: 'A', kind: 'support', w:  0.78, excerpt: 'Total revenue $4.2B vs $3.75B prior-year period (+12.0%).' },
    { from: 'S1', to: 'B', kind: 'support', w:  0.72, excerpt: 'Operating margin of 18.1% compared with 16.4%.' },
    { from: 'S2', to: 'C', kind: 'attack',  w: -0.42, excerpt: 'Q4 FY22 was higher on both revenue and OI.' },
    { from: 'S3', to: 'F', kind: 'support', w:  0.55, excerpt: '“No share repurchases were authorized in Q3.”' },
    // Inter-claim
    { from: 'A',  to: 'C', kind: 'support', w:  0.30, excerpt: 'Implied premise — but record requires full history.' },
    { from: 'B',  to: 'D', kind: 'support', w:  0.18, excerpt: 'Trend, not a guarantee of future expansion.' },
    { from: 'D',  to: 'G', kind: 'support', w:  0.40 },
    { from: 'C',  to: 'G', kind: 'support', w:  0.35 },
    { from: 'E',  to: 'D', kind: 'support', w:  0.22, excerpt: 'AI rollout cited as cost lever.' },
    // Attacks
    { from: 'S2', to: 'D', kind: 'attack',  w: -0.65, excerpt: 'Internal note: opex headwinds in FY25 guidance band.' },
    { from: 'F',  to: 'G', kind: 'attack',  w: -0.20, excerpt: 'Capital return absent — limits peer outperformance case.' },
  ];

  // Stream of judge events (chronological)
  const events = [
    { id: 'e1', t: t(48), kind: 'claim',   target: 'A', text: 'Revenue grew 12% YoY in Q3.',     status: 'pass'  },
    { id: 'e2', t: t(46), kind: 'claim',   target: 'B', text: 'Operating margin reached 18%.',   status: 'pass'  },
    { id: 'e3', t: t(42), kind: 'claim',   target: 'F', text: 'No buybacks announced.',           status: 'pass'  },
    { id: 'e4', t: t(38), kind: 'claim',   target: 'C', text: 'The strongest quarter on record.', status: 'warn',
      reason: 'C-1305: opinion presented as fact. Source contradicts: Q4 FY22 higher.' },
    { id: 'e5', t: t(33), kind: 'claim',   target: 'D', text: 'Margins will expand further next year.', status: 'block',
      reason: 'C-2003: forward-looking projection without disclaimer.' },
    { id: 'e6', t: t(29), kind: 'retract', target: 'D', text: 'Retracted forward-looking sentence.', status: 'retract' },
    { id: 'e7', t: t(22), kind: 'claim',   target: 'E', text: 'Driven by AI tooling rollout.',     status: 'warn',
      reason: 'C-1041: numeric/causal claim missing source citation.' },
    { id: 'e8', t: t(11), kind: 'claim',   target: 'G', text: 'Acme will outperform peers.',       status: 'block',
      reason: 'C-2003: forward-looking projection.' },
    { id: 'e9', t: t(2),  kind: 'judge',   text: 'Re-evaluating support set for claim G…',         status: 'info' },
  ];

  // Generated answer text (for the stream view)
  const stream = [
    { text: 'Acme’s Q3 was strong: ', cls: '' },
    { text: 'revenue grew 12% year-over-year', cls: '', cite: 'S1' },
    { text: ' to $4.2B and ', cls: '' },
    { text: 'operating margin reached 18%', cls: '', cite: 'S1' },
    { text: '. It was ', cls: '' },
    { text: 'the strongest quarter on record', cls: 'warn' },
    { text: '. ', cls: '' },
    { text: 'Margins will expand further next year', cls: 'block' },
    { text: ', driven by AI tooling, and ', cls: '' },
    { text: 'Acme will outperform peers', cls: 'block' },
    { text: '. No buybacks were announced.', cls: '' },
  ];

  return { sources, constraints, nodes, edges, events, stream, sessionId: 'sess_8b3a91' };
})();

// ========= Tiny utility =========
window.fmtTime = function (d) {
  const dd = (n) => String(n).padStart(2, '0');
  return `${dd(d.getHours())}:${dd(d.getMinutes())}:${dd(d.getSeconds())}`;
};
window.relTime = function (d) {
  const s = Math.max(1, Math.round((Date.now() - d.getTime()) / 1000));
  if (s < 60) return s + 's ago';
  return Math.round(s / 60) + 'm ago';
};

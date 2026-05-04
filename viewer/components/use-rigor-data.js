/* global React */
// Reactive hook + context for live daemon data.
// Subscribes to the RIGOR_DATA bridge (mock-data.js) and triggers
// React re-renders on every WebSocket event.

const { createContext, useContext, useState, useEffect, useRef } = React;

const RigorDataContext = createContext(null);

function RigorDataProvider({ children }) {
  const snap = () => ({
    connected:       RIGOR_DATA.connected,
    sessionId:       RIGOR_DATA.sessionId,
    sources:         [...RIGOR_DATA.sources],
    constraints:     [...RIGOR_DATA.constraints],
    nodes:           [...RIGOR_DATA.nodes],
    edges:           [...RIGOR_DATA.edges],
    events:          [...RIGOR_DATA.events],
    stream:          [...RIGOR_DATA.stream],
    streams:         { ...RIGOR_DATA.streams },
    requestLog:      [...RIGOR_DATA.requestLog],
    contextInjected: RIGOR_DATA.contextInjected,
    judgeEntries:    [...RIGOR_DATA.judgeEntries],
    retries:         [...RIGOR_DATA.retries],
    actionGates:     [...RIGOR_DATA.actionGates],
    timeline:        [...RIGOR_DATA.timeline],
    daemonLogs:      [...RIGOR_DATA.daemonLogs],
    governance:      { ...RIGOR_DATA.governance },
    stats:           { ...RIGOR_DATA.stats },
    cost:            { ...RIGOR_DATA.cost },
    sessions:        [...RIGOR_DATA.sessions],
  });

  const [data, setData] = useState(snap);

  useEffect(() => {
    const unsub = RIGOR_DATA.subscribe(() => setData(snap()));
    return unsub;
  }, []);

  return React.createElement(RigorDataContext.Provider, { value: data }, children);
}

function useRigorData() {
  const ctx = useContext(RigorDataContext);
  if (!ctx) throw new Error('useRigorData must be inside RigorDataProvider');
  return ctx;
}

window.RigorDataProvider = RigorDataProvider;
window.useRigorData = useRigorData;

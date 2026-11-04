import * as React from 'react';

import { backTrace, type DrawerTrace, followedTrace, openedTrace, type TraceDrawerState } from '@/routes/traces/explorer/drawer/model';
import { TraceDetailDrawer } from '@/routes/traces/explorer/drawer/TraceDrawer';

import { DrawerTrail, DrawerTrailProvider } from './DrawerTrail';

interface TraceOpener {
  /** Opens a trace in the drawer over the page; focus goes back to `opener` when it closes. */
  openTrace: (trace: DrawerTrace, opener?: HTMLElement | null) => void;
}

const TraceOpenerContext = React.createContext<TraceOpener | null>(null);

/**
 * The trace drawer of an APM page: any part of the page can open a trace in
 * it (a slow request, an error sample) without leaving the page, the same
 * drawer the Traces page opens. A trace opened from a drawer says which
 * drawers lie under it, each a way back.
 */
export function TraceDrawerHost({ children }: { children: React.ReactNode }) {
  const [state, setState] = React.useState<TraceDrawerState | null>(null);
  const returnFocusRef = React.useRef<HTMLElement | null>(null);

  const opener = React.useMemo<TraceOpener>(
    () => ({
      openTrace: (trace, from) => {
        returnFocusRef.current = from ?? null;
        setState(openedTrace(trace));
      },
    }),
    [],
  );

  const close = React.useCallback(() => setState((current) => (current ? { ...current, open: false } : current)), []);

  return (
    <TraceOpenerContext.Provider value={opener}>
      <DrawerTrailProvider>
        {children}
        <TraceDetailDrawer
          state={state}
          onClose={close}
          trail={<DrawerTrail over="trace" onLeave={close} />}
          onFollow={(trace) => setState((current) => (current ? followedTrace(current, trace) : openedTrace(trace)))}
          onBack={() => setState((current) => (current ? backTrace(current) : current))}
          pageHref={(trace) => `/traces/${encodeURIComponent(trace.traceId)}`}
          returnFocusRef={returnFocusRef}
        />
      </DrawerTrailProvider>
    </TraceOpenerContext.Provider>
  );
}

export function useTraceOpener(): TraceOpener {
  const opener = React.useContext(TraceOpenerContext);
  if (!opener) throw new Error('useTraceOpener needs a TraceDrawerHost');
  return opener;
}

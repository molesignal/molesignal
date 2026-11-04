import { create } from 'zustand';

interface TraceViewPrefs {
  /** The critical path is drawn over the waterfall and the flame graph. */
  criticalPath: boolean;
  setCriticalPath: (criticalPath: boolean) => void;
}

/**
 * What the reader turned on in one trace's view and keeps on in the next: a
 * drawer stepping through a list opens each trace anew.
 */
export const useTraceViewPrefs = create<TraceViewPrefs>((set) => ({
  criticalPath: false,
  setCriticalPath: (criticalPath) => set({ criticalPath }),
}));

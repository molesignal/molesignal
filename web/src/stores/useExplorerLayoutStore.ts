import { create } from 'zustand';
import { persist } from 'zustand/middleware';

/**
 * Which optional parts of the Logs and Traces explorers the user keeps open.
 *
 * The histogram above the results can be closed to give the rows more room, and
 * the choice is remembered per explorer, so the page opens the way it was left.
 *
 * The query editor is not here on purpose: on both explorers it always starts
 * folded into the bar and opens when there is a query to look at.
 */
interface ExplorerLayoutState {
  logsHistogramVisible: boolean;
  tracesHistogramVisible: boolean;
  setLogsHistogramVisible: (visible: boolean) => void;
  setTracesHistogramVisible: (visible: boolean) => void;
}

export const EXPLORER_LAYOUT_STORAGE_KEY = 'molesignal.explorer.layout.v1';

export const useExplorerLayoutStore = create<ExplorerLayoutState>()(
  persist(
    (set) => ({
      logsHistogramVisible: true,
      tracesHistogramVisible: true,
      setLogsHistogramVisible: (logsHistogramVisible) => set({ logsHistogramVisible }),
      setTracesHistogramVisible: (tracesHistogramVisible) => set({ tracesHistogramVisible }),
    }),
    {
      name: EXPLORER_LAYOUT_STORAGE_KEY,
      // Keep only what this store still owns; an older build also saved the editor state.
      partialize: (state) => ({
        logsHistogramVisible: state.logsHistogramVisible,
        tracesHistogramVisible: state.tracesHistogramVisible,
      }),
    },
  ),
);

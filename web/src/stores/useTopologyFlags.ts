import { create } from 'zustand';

/**
 * Per-node visual flags for the topology view (web-ui-polish).
 *
 * Hysteresis for the red ring: `enter` threshold (0.05) is higher than the
 * `exit` threshold (0.045), so a node oscillating at the boundary
 * (err_rate 0.049 ↔ 0.051) never flickers. The store is purely UI state —
 * it does not block render and does not need to be persisted.
 */
export const RED_RING_ENTER = 0.05;
export const RED_RING_EXIT = 0.045;

interface TopologyFlagsState {
  redRing: Record<string, boolean>;
  applyHysteresis: (nodes: Array<{ id: string; error_rate: number }>) => void;
  reset: () => void;
}

export const useTopologyFlags = create<TopologyFlagsState>((set, get) => ({
  redRing: {},
  applyHysteresis: (nodes) => {
    const prev = get().redRing;
    const next: Record<string, boolean> = {};
    let changed = false;
    for (const n of nodes) {
      next[n.id] = computeRedRing(prev[n.id] ?? false, n.error_rate);
      if (next[n.id] !== (prev[n.id] ?? false)) changed = true;
    }
    // Drop ids no longer present.
    for (const id of Object.keys(prev)) {
      if (!(id in next)) changed = true;
    }
    if (changed) set({ redRing: next });
  },
  reset: () => set({ redRing: {} }),
}));

/**
 * Pure hysteresis decision — exported so vitest can cover the boundary
 * cases without booting the zustand store.
 */
export function computeRedRing(currentlyOn: boolean, errRate: number): boolean {
  if (currentlyOn) {
    // Stay on until err_rate drops below the EXIT threshold.
    return errRate >= RED_RING_EXIT;
  }
  // Turn on once err_rate hits the ENTER threshold.
  return errRate >= RED_RING_ENTER;
}

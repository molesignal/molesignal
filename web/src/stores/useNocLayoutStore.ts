import { create } from 'zustand';
import { persist } from 'zustand/middleware';

/**
 * NOC wallboard layout — which panels show, in what order, at what width.
 * Persisted to localStorage so a team's wallboard configuration survives
 * reloads. Drag-and-drop would need a DnD dependency the app doesn't ship; the
 * edit bar (reorder / resize / show-hide / presets) gives the same control
 * without one. Visual identity (forced dark, oversized type) is untouched —
 * only the panel grid is configurable.
 */
export type NocPanelId = 'traffic' | 'incidents' | 'topology' | 'health';
export type NocPanelSpan = 2 | 4;
export type NocPreset = 'platform' | 'sre' | 'executive';

export interface NocPanelConfig {
  id: NocPanelId;
  visible: boolean;
  span: NocPanelSpan;
}

const DEFAULT_LAYOUT: NocPanelConfig[] = [
  { id: 'traffic', visible: true, span: 2 },
  { id: 'incidents', visible: true, span: 2 },
  { id: 'topology', visible: true, span: 2 },
  { id: 'health', visible: true, span: 2 },
];

const PRESETS: Record<NocPreset, NocPanelConfig[]> = {
  // Platform ops: topology + health lead, traffic + incidents support.
  platform: [
    { id: 'topology', visible: true, span: 4 },
    { id: 'health', visible: true, span: 2 },
    { id: 'traffic', visible: true, span: 2 },
    { id: 'incidents', visible: true, span: 4 },
  ],
  // SRE: incidents first, everything in even halves for fast scanning.
  sre: [
    { id: 'incidents', visible: true, span: 2 },
    { id: 'health', visible: true, span: 2 },
    { id: 'topology', visible: true, span: 2 },
    { id: 'traffic', visible: true, span: 2 },
  ],
  // Executive: only the two big-picture panels, full width.
  executive: [
    { id: 'incidents', visible: true, span: 4 },
    { id: 'topology', visible: true, span: 4 },
    { id: 'traffic', visible: false, span: 2 },
    { id: 'health', visible: false, span: 2 },
  ],
};

const clone = (layout: NocPanelConfig[]): NocPanelConfig[] => layout.map((p) => ({ ...p }));

interface NocLayoutState {
  panels: NocPanelConfig[];
  move: (id: NocPanelId, dir: -1 | 1) => void;
  toggleVisible: (id: NocPanelId) => void;
  cycleSpan: (id: NocPanelId) => void;
  applyPreset: (preset: NocPreset) => void;
  reset: () => void;
}

export const useNocLayoutStore = create<NocLayoutState>()(
  persist(
    (set) => ({
      panels: clone(DEFAULT_LAYOUT),
      move: (id, dir) =>
        set((s) => {
          const idx = s.panels.findIndex((p) => p.id === id);
          const next = idx + dir;
          if (idx < 0 || next < 0 || next >= s.panels.length) return s;
          const panels = [...s.panels];
          const [item] = panels.splice(idx, 1);
          panels.splice(next, 0, item!);
          return { panels };
        }),
      toggleVisible: (id) =>
        set((s) => ({ panels: s.panels.map((p) => (p.id === id ? { ...p, visible: !p.visible } : p)) })),
      cycleSpan: (id) =>
        set((s) => ({ panels: s.panels.map((p) => (p.id === id ? { ...p, span: p.span === 2 ? 4 : 2 } : p)) })),
      applyPreset: (preset) => set({ panels: clone(PRESETS[preset]) }),
      reset: () => set({ panels: clone(DEFAULT_LAYOUT) }),
    }),
    { name: 'molesignal.noc-layout.v1', version: 1 },
  ),
);

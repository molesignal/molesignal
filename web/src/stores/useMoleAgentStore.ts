import { create } from 'zustand';

/**
 * Shell-level Mole Agent panel state. The agent is a right-side slide-out
 * overlay available from any route (Topbar ✨ button / ⌘J) so an operator can
 * ask a question without losing the page they're investigating — see
 * `shell/MoleAgentPanel.tsx`. Kept intentionally tiny: the panel reads the
 * live route + time window for context itself, so all this store owns is the
 * open/closed bit plus an optional pre-filled question (`seed`) for future
 * "Ask Mole Agent about this signal" entry points.
 */
interface MoleAgentState {
  isOpen: boolean;
  seed: string | null;
  open: (seed?: string) => void;
  close: () => void;
  toggle: () => void;
}

export const useMoleAgentStore = create<MoleAgentState>((set, get) => ({
  isOpen: false,
  seed: null,
  open: (seed) => set({ isOpen: true, seed: seed ?? null }),
  close: () => set({ isOpen: false }),
  toggle: () => set({ isOpen: !get().isOpen }),
}));

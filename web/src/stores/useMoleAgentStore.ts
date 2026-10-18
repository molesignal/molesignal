import { create } from 'zustand';

import type { MessageReference } from '@/api/agent/chat';

/**
 * A new chat asked for from elsewhere in the product: the question to prefill
 * (never sent on the person's behalf), and what the chat starts with: the team
 * it is for, what it investigates, and what is attached to the question.
 */
export interface ChatIntent {
  prompt: string;
  /** The team the chat is for; none for a personal chat. */
  teamId?: string | null;
  /** What the chat investigates: an incident it is started from. */
  subject?: { kind: 'incident'; id: string } | null;
  references?: MessageReference[];
}

/**
 * Shell-level Mole Agent panel state. The agent is a right-side slide-out
 * overlay available from any route (Topbar ✨ button / ⌘J) so an operator can
 * ask a question without losing the page they're investigating — see
 * `shell/MoleAgentPanel.tsx`. Kept intentionally tiny: the panel reads the
 * live route + time window for context itself, so all this store owns is the
 * open/closed bit, an optional pre-filled question (`seed`), and a new chat
 * asked for by a page (`intent`).
 */
interface MoleAgentState {
  isOpen: boolean;
  seed: string | null;
  intent: ChatIntent | null;
  open: (seed?: string) => void;
  /** Open the panel on a new chat that starts as `intent` says. */
  start: (intent: ChatIntent) => void;
  close: () => void;
  toggle: () => void;
}

export const useMoleAgentStore = create<MoleAgentState>((set, get) => ({
  isOpen: false,
  seed: null,
  intent: null,
  open: (seed) => set({ isOpen: true, seed: seed ?? null }),
  start: (intent) => set({ isOpen: true, intent }),
  close: () => set({ isOpen: false }),
  toggle: () => set({ isOpen: !get().isOpen }),
}));

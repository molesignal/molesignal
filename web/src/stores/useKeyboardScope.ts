import { create } from 'zustand';

export type Scope = 'global' | 'palette' | 'drawer' | 'chart-brush' | 'editor' | 'help-overlay';

interface ScopeState {
  stack: Scope[];
  push: (scope: Scope) => void;
  pop: () => Scope | undefined;
  peek: () => Scope;
  reset: () => void;
}

export const useKeyboardScope = create<ScopeState>((set, get) => ({
  stack: ['global'],
  push: (scope) => set({ stack: [...get().stack, scope] }),
  pop: () => {
    const stack = get().stack;
    if (stack.length <= 1) return undefined;
    const next = stack.slice(0, -1);
    set({ stack: next });
    return stack[stack.length - 1];
  },
  peek: () => {
    const stack = get().stack;
    return stack[stack.length - 1] ?? 'global';
  },
  reset: () => set({ stack: ['global'] }),
}));

import * as React from 'react';

import { type Scope, useKeyboardScope } from '@/stores/useKeyboardScope';
import { useThemeStore } from '@/stores/useThemeStore';

export interface Binding {
  /** Key combo string. Use modified combos only, e.g. `mod+k`, `mod+alt+t`, `mod+esc`. Lowercase recommended. */
  keys: string;
  description: string;
  handler: (e: KeyboardEvent) => void;
  /** Optional category for the help overlay. */
  category?: string;
}

type Registry = Map<Scope, Map<string, Binding>>;

interface Controller {
  registerBindings: (scope: Scope, bindings: Binding[]) => () => void;
  listBindings: (scope: Scope) => Binding[];
  pendingChord: string | null;
}

const ControllerContext = React.createContext<Controller | null>(null);

const CHORD_TIMEOUT_MS = 800;

export function KeyboardProvider({ children }: { children: React.ReactNode }) {
  const registry = React.useRef<Registry>(new Map());
  const pendingChordRef = React.useRef<string | null>(null);
  const chordTimerRef = React.useRef<number | null>(null);

  const cancelChord = React.useCallback(() => {
    pendingChordRef.current = null;
    if (chordTimerRef.current !== null) {
      window.clearTimeout(chordTimerRef.current);
      chordTimerRef.current = null;
    }
  }, []);

  React.useLayoutEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (!useThemeStore.getState().keyboardShortcutsEnabled) {
        cancelChord();
        return;
      }
      const combo = normalizeEvent(e);
      const scope = useKeyboardScope.getState().peek();
      const hasSystemModifier = e.metaKey || e.ctrlKey || e.altKey;

      // Global shortcuts must be modified combos so normal typing in Monaco,
      // search inputs, tables, and virtualized rows can never be captured.
      if (!hasSystemModifier) {
        cancelChord();
        return;
      }

      // Chord handling: if previous key was a chord-leader (e.g. `g`), the next key becomes `g <next>`.
      const pending = pendingChordRef.current;
      const lookup = pending ? `${pending} ${combo}` : combo;

      const scopeBindings = registry.current.get(scope) ?? new Map();
      const globalBindings = registry.current.get('global') ?? new Map();
      const binding = scopeBindings.get(lookup) ?? globalBindings.get(lookup);

      if (binding) {
        cancelChord();
        e.preventDefault();
        binding.handler(e);
        return;
      }

      // Maybe this is a chord leader.
      if (!pending && isChordLeader(combo, scope)) {
        pendingChordRef.current = combo;
        chordTimerRef.current = window.setTimeout(cancelChord, CHORD_TIMEOUT_MS);
        return;
      }

      // Pending chord didn't match — drop it and try the new key as a standalone.
      if (pending) {
        cancelChord();
        const standalone = scopeBindings.get(combo) ?? globalBindings.get(combo);
        if (standalone) {
          e.preventDefault();
          standalone.handler(e);
        }
      }
    };
    window.addEventListener('keydown', onKeyDown, true);
    return () => window.removeEventListener('keydown', onKeyDown, true);
  }, [cancelChord]);

  const controller: Controller = {
    registerBindings: (scope, bindings) => {
      let bucket = registry.current.get(scope);
      if (!bucket) {
        bucket = new Map();
        registry.current.set(scope, bucket);
      }
      const added: string[] = [];
      for (const b of bindings) {
        if (bucket.has(b.keys)) {
          // duplicate: first registration wins
          continue;
        }
        bucket.set(b.keys, b);
        added.push(b.keys);
      }
      return () => {
        const bk = registry.current.get(scope);
        if (!bk) return;
        for (const k of added) bk.delete(k);
      };
    },
    listBindings: (scope) => {
      const out: Binding[] = [];
      const global = registry.current.get('global');
      if (global) out.push(...Array.from(global.values()));
      if (scope !== 'global') {
        const s = registry.current.get(scope);
        if (s) out.push(...Array.from(s.values()));
      }
      return out;
    },
    pendingChord: pendingChordRef.current,
  };

  return <ControllerContext.Provider value={controller}>{children}</ControllerContext.Provider>;
}

export function useKeyboard(): Controller {
  const ctx = React.useContext(ControllerContext);
  if (!ctx) throw new Error('useKeyboard must be used within KeyboardProvider');
  return ctx;
}

/**
 * Register bindings declaratively from a component. Handle is returned only as
 * a stable reference; the cleanup is automatic on unmount.
 */
export function useBindings(scope: Scope, bindings: Binding[]) {
  const ctrl = useKeyboard();
  React.useLayoutEffect(() => ctrl.registerBindings(scope, bindings), [ctrl, scope, bindings]);
}

/**
 * Push a scope onto the stack while a component is mounted. Cleanup pops it.
 */
export function useScope(scope: Scope, active = true) {
  const push = useKeyboardScope((s) => s.push);
  const pop = useKeyboardScope((s) => s.pop);
  React.useEffect(() => {
    if (!active) return;
    push(scope);
    return () => {
      pop();
    };
  }, [scope, active, push, pop]);
}

function normalizeEvent(e: KeyboardEvent): string {
  const parts: string[] = [];
  if (e.metaKey || e.ctrlKey) parts.push('mod');
  if (e.altKey) parts.push('alt');
  if (e.shiftKey && e.key.length > 1) parts.push('shift');
  const k = e.key;
  let key: string;
  if (k === 'Escape') key = 'esc';
  else if (k === 'Enter') key = 'enter';
  else if (k === 'ArrowDown') key = 'down';
  else if (k === 'ArrowUp') key = 'up';
  else if (k === 'ArrowLeft') key = 'left';
  else if (k === 'ArrowRight') key = 'right';
  else if (k === ' ') key = 'space';
  else key = k.toLowerCase();
  parts.push(key);
  return parts.join('+');
}

function isChordLeader(combo: string, scope: Scope): boolean {
  // Only `g` is a chord leader in the global scope. Extendable per scope later.
  if (scope === 'global' && combo === 'g') return true;
  return false;
}

import { describe, expect, it } from 'vitest';

import { DEFAULT_WINDOW, resolveExpr, resolveWindow, useTimeStore } from '@/stores/useTimeStore';

describe('useTimeStore relative window', () => {
  it('resolveWindow recomputes against `now` on every call', () => {
    const t0 = new Date('2026-05-23T10:00:00Z');
    const t1 = new Date('2026-05-23T11:00:00Z');
    const w = { from: 'now-1h', to: 'now', mode: 'relative' as const };
    const a = resolveWindow(w, t0);
    const b = resolveWindow(w, t1);
    expect(b.to.getTime() - a.to.getTime()).toBe(3_600_000);
  });

  it('resolveExpr parses ISO absolute strings', () => {
    const d = resolveExpr('2026-05-23T10:00:00Z', new Date());
    expect(d.toISOString()).toBe('2026-05-23T10:00:00.000Z');
  });
});

describe('anchor pin/unpin', () => {
  it('setAnchor + clearAnchor round-trip', () => {
    const store = useTimeStore.getState();
    store.setAnchor({ at: '2026-05-23T10:00:00Z' });
    expect(useTimeStore.getState().anchor?.at).toBe('2026-05-23T10:00:00Z');
    store.clearAnchor();
    expect(useTimeStore.getState().anchor).toBeNull();
  });

  it('togglePin idempotent on same timestamp', () => {
    const store = useTimeStore.getState();
    store.clearAnchor();
    store.togglePin('2026-05-23T10:00:00Z');
    expect(useTimeStore.getState().anchor?.at).toBe('2026-05-23T10:00:00Z');
    store.togglePin('2026-05-23T10:00:00Z');
    expect(useTimeStore.getState().anchor).toBeNull();
  });
});

describe('DEFAULT_WINDOW', () => {
  it('is relative now-1h..now', () => {
    expect(DEFAULT_WINDOW.mode).toBe('relative');
    expect(DEFAULT_WINDOW.from).toBe('now-1h');
    expect(DEFAULT_WINDOW.to).toBe('now');
  });
});

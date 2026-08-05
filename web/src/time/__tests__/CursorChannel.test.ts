import { renderHook } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { publishCursor, useCursorChannel } from '@/time/CursorChannel';

describe('CursorChannel', () => {
  it('publishes to subscribers in the same scope', () => {
    const { result } = renderHook(() => useCursorChannel('s1'));
    const cb = vi.fn();
    const unsub = result.current.subscribe(cb);
    publishCursor('s1', 1234, 'src');
    expect(cb).toHaveBeenCalledWith(1234, 'src');
    unsub();
  });

  it('emits source id in the callback payload so caller can filter reflow', () => {
    const { result } = renderHook(() => useCursorChannel('s2'));
    const cb = vi.fn();
    const unsub = result.current.subscribe(cb);
    publishCursor('s2', 99, 'self');
    expect(cb.mock.calls[0]?.[1]).toBe('self');
    unsub();
  });

  it('isolates scopes', () => {
    const a = renderHook(() => useCursorChannel('scopeA')).result.current;
    const b = renderHook(() => useCursorChannel('scopeB')).result.current;
    const cbA = vi.fn();
    const cbB = vi.fn();
    const u1 = a.subscribe(cbA);
    const u2 = b.subscribe(cbB);
    publishCursor('scopeA', 1, 'x');
    expect(cbA).toHaveBeenCalledTimes(1);
    expect(cbB).not.toHaveBeenCalled();
    u1();
    u2();
  });
});

import { act, renderHook } from '@testing-library/react';
import type { MutableRefObject } from 'react';
import type uPlot from 'uplot';
import { describe, expect, it, vi } from 'vitest';

import { publishCursor, useCursorChannel } from '@/time/CursorChannel';
import { useCursorSync } from '@/viz/timeseries/cursorSync';

describe('useCursorSync', () => {
  it('subscribes and publishes only while shared crosshair is enabled', () => {
    const plot = {
      cursor: { top: 7 },
      valToPos: vi.fn(() => 42),
      setCursor: vi.fn(),
    } as unknown as uPlot;
    const plotRef = { current: plot } as MutableRefObject<uPlot | null>;
    const testId = crypto.randomUUID();
    const disabledScopeId = `cursor-sync-disabled-${testId}`;
    const sharedScopeId = `cursor-sync-shared-${testId}`;
    const listener = vi.fn();
    const channel = renderHook(
      () => useCursorChannel(sharedScopeId),
    ).result.current;
    const unsubscribe = channel.subscribe(listener);
    const hook = renderHook(
      ({ scopeId, enabled }) => useCursorSync(plotRef, scopeId, enabled),
      { initialProps: { scopeId: disabledScopeId, enabled: false } },
    );
    vi.mocked(plot.setCursor).mockClear();

    act(() => {
      hook.result.current.onCursorMove(10);
      publishCursor(sharedScopeId, 11, 'another-chart');
    });
    expect(listener).toHaveBeenCalledTimes(1);
    expect(plot.setCursor).not.toHaveBeenCalled();

    hook.rerender({ scopeId: sharedScopeId, enabled: true });
    act(() => publishCursor(sharedScopeId, 12, 'another-chart'));
    expect(plot.valToPos).toHaveBeenCalledWith(12, 'x');
    expect(plot.setCursor).toHaveBeenCalledWith(
      { left: 42, top: 7 },
      false,
    );

    listener.mockClear();
    act(() => hook.result.current.onCursorMove(13));
    expect(listener).toHaveBeenCalledWith(13, hook.result.current.sourceId);
    unsubscribe();
  });
});

import { act, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { useAutoSave } from './useAutoSave';

describe('useAutoSave', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('debounces and saves each distinct draft once', async () => {
    const save = vi.fn().mockResolvedValue(undefined);
    const { rerender } = renderHook(
      ({ fingerprint }) =>
        useAutoSave({
          fingerprint,
          enabled: true,
          busy: false,
          delay: 700,
          save,
        }),
      { initialProps: { fingerprint: 'first' } },
    );

    await act(() => vi.advanceTimersByTimeAsync(699));
    expect(save).not.toHaveBeenCalled();
    await act(() => vi.advanceTimersByTimeAsync(1));
    expect(save).toHaveBeenCalledTimes(1);

    rerender({ fingerprint: 'second' });
    await act(() => vi.advanceTimersByTimeAsync(700));
    expect(save).toHaveBeenCalledTimes(2);
  });

  it('waits for explicit retry after a failed save', async () => {
    const save = vi.fn().mockRejectedValue(new Error('offline'));
    const { result, rerender } = renderHook(
      ({ busy }) =>
        useAutoSave({
          fingerprint: 'failed-draft',
          enabled: true,
          busy,
          delay: 700,
          save,
        }),
      { initialProps: { busy: false } },
    );

    await act(() => vi.advanceTimersByTimeAsync(700));
    expect(save).toHaveBeenCalledTimes(1);

    rerender({ busy: true });
    rerender({ busy: false });
    await act(() => vi.advanceTimersByTimeAsync(700));
    expect(save).toHaveBeenCalledTimes(1);

    act(() => result.current());
    expect(save).toHaveBeenCalledTimes(2);
  });
});

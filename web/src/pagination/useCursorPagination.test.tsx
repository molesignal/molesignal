import { act, renderHook } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { useCursorPagination } from './useCursorPagination';

describe('useCursorPagination', () => {
  it('invalidates the cursor synchronously when query context changes', () => {
    const { result, rerender } = renderHook(
      ({ contextKey }) => useCursorPagination({ contextKey }),
      { initialProps: { contextKey: 'range-a' } },
    );

    act(() => {
      result.current.goNext({
        next_cursor: 'page-2',
        previous_cursor: null,
      });
    });
    expect(result.current.cursor).toBe('page-2');

    rerender({ contextKey: 'range-b' });
    expect(result.current.cursor).toBeNull();

    rerender({ contextKey: 'range-a' });
    expect(result.current.cursor).toBeNull();
  });

  it('resets the cursor when page size changes', () => {
    const { result } = renderHook(() =>
      useCursorPagination({ contextKey: 'query', defaultPageSize: 20 }),
    );
    act(() => {
      result.current.goNext({
        next_cursor: 'page-2',
        previous_cursor: null,
      });
      result.current.setPageSize(50);
    });

    expect(result.current.pageSize).toBe(50);
    expect(result.current.cursor).toBeNull();
  });
});

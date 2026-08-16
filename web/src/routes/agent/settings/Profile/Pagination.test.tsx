import { act, renderHook } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { useProfilePagination } from './Pagination';

describe('useProfilePagination', () => {
  it('paginates profiles and resets to the first page when page size changes', () => {
    const profiles = Array.from({ length: 23 }, (_, index) => index + 1);
    const { result } = renderHook(() => useProfilePagination(profiles));

    expect(result.current.pageItems).toEqual(profiles.slice(0, 10));
    expect(result.current.pageCount).toBe(3);

    act(() => result.current.onPageChange(3));
    expect(result.current.pageItems).toEqual(profiles.slice(20));

    act(() => result.current.onPageSizeChange(20));
    expect(result.current.page).toBe(1);
    expect(result.current.pageCount).toBe(2);
    expect(result.current.pageItems).toEqual(profiles.slice(0, 20));
  });

  it('moves back to the last available page when the list shrinks', () => {
    const profiles = Array.from({ length: 21 }, (_, index) => index + 1);
    const { result, rerender } = renderHook(
      ({ items }: { items: number[] }) => useProfilePagination(items),
      { initialProps: { items: profiles } },
    );

    act(() => result.current.onPageChange(3));
    rerender({ items: profiles.slice(0, 15) });

    expect(result.current.page).toBe(2);
    expect(result.current.pageItems).toEqual(profiles.slice(10, 15));
  });
});

import { act, render, renderHook, screen, waitFor } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import '@/i18n';

import { StreamPagination, useStreamPagination } from './StreamPagination';

describe('useStreamPagination', () => {
  it('paginates filtered streams and resets when filters change', async () => {
    const streams = Array.from({ length: 45 }, (_, index) => index + 1);
    const { result, rerender } = renderHook(
      ({ items, resetKey }) => useStreamPagination(items, resetKey),
      { initialProps: { items: streams, resetKey: 'all' } },
    );

    expect(result.current.pageCount).toBe(3);
    expect(result.current.pageItems).toEqual(streams.slice(0, 20));

    act(() => result.current.onPageChange(3));
    expect(result.current.pageItems).toEqual(streams.slice(40));

    rerender({ items: streams.slice(0, 8), resetKey: 'logs' });
    await waitFor(() => expect(result.current.page).toBe(1));
    expect(result.current.pageItems).toEqual(streams.slice(0, 8));
  });

  it('hides empty and single short pages', () => {
    const props = {
      page: 1,
      pageCount: 1,
      pageSize: 20,
      onPageChange: () => undefined,
      onPageSizeChange: () => undefined,
    };
    const { rerender } = render(<StreamPagination {...props} total={0} />);

    expect(screen.queryByRole('navigation')).toBeNull();

    rerender(<StreamPagination {...props} total={20} />);
    expect(screen.queryByRole('navigation')).toBeNull();

    rerender(<StreamPagination {...props} total={21} pageCount={2} />);
    const pagination = screen.getByRole('navigation');
    expect(pagination.className).toContain('border-0');
    expect(pagination.className).toContain('w-fit');
  });
});

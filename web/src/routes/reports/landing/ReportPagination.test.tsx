import { act, render, renderHook, screen, waitFor } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import '@/i18n';

import { ReportPagination, useReportPagination } from './ReportPagination';

describe('useReportPagination', () => {
  it('paginates items and resets after filters or page size change', async () => {
    const allItems = Array.from({ length: 45 }, (_, index) => index + 1);
    const { result, rerender } = renderHook(
      ({ items, resetKey }) => useReportPagination(items, resetKey),
      { initialProps: { items: allItems, resetKey: 'all' } },
    );

    expect(result.current.pageCount).toBe(3);
    expect(result.current.pageItems).toEqual(allItems.slice(0, 20));

    act(() => result.current.onPageChange(3));
    expect(result.current.pageItems).toEqual(allItems.slice(40));

    rerender({ items: allItems.slice(0, 8), resetKey: 'filtered' });
    await waitFor(() => expect(result.current.page).toBe(1));
    expect(result.current.pageItems).toEqual(allItems.slice(0, 8));

    act(() => result.current.onPageSizeChange(50));
    expect(result.current.page).toBe(1);
    expect(result.current.pageCount).toBe(1);
  });

  it('hides a single short page and clusters multi-page controls', () => {
    const props = {
      page: 1,
      pageCount: 1,
      pageSize: 20,
      total: 20,
      onPageChange: () => undefined,
      onPageSizeChange: () => undefined,
    };
    const { rerender } = render(<ReportPagination {...props} />);

    expect(screen.queryByRole('navigation')).toBeNull();

    rerender(
      <ReportPagination {...props} pageCount={2} total={21} />,
    );
    const pagination = screen.getByRole('navigation');
    expect(pagination.className).toContain('ml-auto');
    expect(pagination.className).toContain('w-fit');
    expect(pagination.className).toContain('border-0');
  });
});

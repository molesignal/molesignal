import {
  act,
  render,
  renderHook,
  screen,
  waitFor,
} from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import '@/i18n';

import {
  ExtendTablePagination,
  useExtendTablePagination,
} from './Pagination';

describe('useExtendTablePagination', () => {
  it('paginates tables and resets when filters change', async () => {
    const tables = Array.from({ length: 23 }, (_, index) => index + 1);
    const { result, rerender } = renderHook(
      ({ items, resetKey }) => useExtendTablePagination(items, resetKey),
      { initialProps: { items: tables, resetKey: 'all' } },
    );

    expect(result.current.pageItems).toEqual(tables.slice(0, 10));
    expect(result.current.pageCount).toBe(3);

    act(() => result.current.onPageChange(3));
    expect(result.current.pageItems).toEqual(tables.slice(20));

    rerender({ items: tables.slice(0, 12), resetKey: 'used' });
    await waitFor(() => expect(result.current.page).toBe(1));
    expect(result.current.pageItems).toEqual(tables.slice(0, 10));
  });

  it('returns to the last available page when the list shrinks', async () => {
    const tables = Array.from({ length: 21 }, (_, index) => index + 1);
    const { result, rerender } = renderHook(
      ({ items }) => useExtendTablePagination(items, 'all'),
      { initialProps: { items: tables } },
    );

    act(() => result.current.onPageChange(3));
    rerender({ items: tables.slice(0, 15) });

    await waitFor(() => expect(result.current.page).toBe(2));
    expect(result.current.pageItems).toEqual(tables.slice(10, 15));
  });
});

describe('ExtendTablePagination', () => {
  it('only renders for multiple pages and stays cardless', () => {
    const props = {
      page: 1,
      pageCount: 1,
      pageSize: 10,
      total: 10,
      onPageChange: vi.fn(),
      onPageSizeChange: vi.fn(),
    };
    const { rerender } = render(<ExtendTablePagination {...props} />);

    expect(screen.queryByRole('navigation')).toBeNull();

    rerender(
      <ExtendTablePagination {...props} pageCount={2} total={11} />,
    );
    const pagination = screen.getByRole('navigation');
    expect(pagination.className).toContain('bg-transparent');
    expect(pagination.className).not.toMatch(/rounded-lg|shadow|bg-bg-1/);
  });
});

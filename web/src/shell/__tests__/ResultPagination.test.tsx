import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { ResultPagination } from '@/shell/ResultPagination';

afterEach(() => {
  cleanup();
});

function renderPagination(page: number, pageCount: number) {
  const onPageChange = vi.fn();
  render(
    <ResultPagination
      page={page}
      pageCount={pageCount}
      pageSize={50}
      pageSizeOptions={[25, 50, 100]}
      pageLabel={`${page} / ${pageCount}`}
      ariaLabel="Results pagination"
      pageSizeAriaLabel="Rows per page"
      firstAriaLabel="First page"
      previousAriaLabel="Previous page"
      nextAriaLabel="Next page"
      lastAriaLabel="Last page"
      onPageChange={onPageChange}
      onPageSizeChange={vi.fn()}
    />,
  );
  return onPageChange;
}

describe('ResultPagination', () => {
  it('keeps the query footer height and navigates to adjacent pages', () => {
    const onPageChange = renderPagination(2, 4);

    const pagination = screen.getByRole('navigation', { name: 'Results pagination' });
    expect(pagination.className).toContain('h-11');
    expect(screen.getByText('2 / 4')).not.toBeNull();
    const pageSize = screen.getByRole('combobox', { name: 'Rows per page' });
    expect(pageSize.className).toContain('border-0');
    expect(pageSize.className).toContain('bg-transparent');

    fireEvent.click(screen.getByRole('button', { name: 'First page' }));
    fireEvent.click(screen.getByRole('button', { name: 'Previous page' }));
    fireEvent.click(screen.getByRole('button', { name: 'Next page' }));
    fireEvent.click(screen.getByRole('button', { name: 'Last page' }));

    expect(onPageChange).toHaveBeenNthCalledWith(1, 1);
    expect(onPageChange).toHaveBeenNthCalledWith(2, 1);
    expect(onPageChange).toHaveBeenNthCalledWith(3, 3);
    expect(onPageChange).toHaveBeenNthCalledWith(4, 4);
  });

  it('disables navigation at the first and final page', () => {
    const { rerender } = render(
      <ResultPagination
        page={1}
        pageCount={3}
        pageSize={50}
        pageSizeOptions={[50]}
        pageLabel="1 / 3"
        ariaLabel="Results pagination"
        pageSizeAriaLabel="Rows per page"
        firstAriaLabel="First page"
        previousAriaLabel="Previous page"
        nextAriaLabel="Next page"
        lastAriaLabel="Last page"
        onPageChange={vi.fn()}
        onPageSizeChange={vi.fn()}
      />,
    );
    expect((screen.getByRole('button', { name: 'Previous page' }) as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole('button', { name: 'First page' }) as HTMLButtonElement).disabled).toBe(true);

    rerender(
      <ResultPagination
        page={3}
        pageCount={3}
        pageSize={50}
        pageSizeOptions={[50]}
        pageLabel="3 / 3"
        ariaLabel="Results pagination"
        pageSizeAriaLabel="Rows per page"
        firstAriaLabel="First page"
        previousAriaLabel="Previous page"
        nextAriaLabel="Next page"
        lastAriaLabel="Last page"
        onPageChange={vi.fn()}
        onPageSizeChange={vi.fn()}
      />,
    );
    expect((screen.getByRole('button', { name: 'Next page' }) as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole('button', { name: 'Last page' }) as HTMLButtonElement).disabled).toBe(true);
  });
});

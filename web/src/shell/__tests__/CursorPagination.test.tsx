import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { CursorPagination } from '@/shell/CursorPagination';

describe('CursorPagination', () => {
  it('only enables directions represented by server cursors', () => {
    const previous = vi.fn();
    const next = vi.fn();
    render(
      <CursorPagination
        pageSize={20}
        pageSizeOptions={[20, 50]}
        hasPrevious={false}
        hasNext
        ariaLabel="Trace pagination"
        pageSizeAriaLabel="Traces per page"
        previousLabel="Previous"
        nextLabel="Next"
        onPrevious={previous}
        onNext={next}
        onPageSizeChange={vi.fn()}
      />,
    );

    expect(screen.getByRole('button', { name: 'Previous' })).toHaveProperty(
      'disabled',
      true,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Next' }));
    expect(previous).not.toHaveBeenCalled();
    expect(next).toHaveBeenCalledOnce();
    expect(screen.queryByText(/\d+\s*\/\s*\d+/)).toBeNull();
  });
});

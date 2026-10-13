import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';

import '@/i18n';

import { ProductState } from './states';

afterEach(cleanup);

describe('ProductState', () => {
  it('shows loading as the pulse travelling a line, not a spinner', () => {
    const { container } = render(<ProductState variant="loading" title="Loading" />);
    expect(screen.getByRole('status')).not.toBeNull();
    expect(container.querySelector('path.ms-pulse-travel')).not.toBeNull();
    expect(container.querySelector('.animate-spin')).toBeNull();
  });

  it('shows empty as a flat line with one beat', () => {
    const { container } = render(<ProductState variant="empty" title="Nothing yet" />);
    expect(container.querySelector('path.ms-pulse-draw')).not.toBeNull();
  });

  it('keeps a tinted glyph for the states that need attention', () => {
    for (const [variant, tile] of [
      ['error', 'bg-red-dim'],
      ['permission-denied', 'bg-red-dim'],
      ['backend-pending', 'bg-[var(--yellow-tint)]'],
    ] as const) {
      const { container, unmount } = render(<ProductState variant={variant} title="Attention" />);
      // By attribute: a class that names a variable (`bg-[var(--x)]`) is not a valid `.class` selector.
      expect(container.querySelector(`[class~="${tile}"] svg`)).not.toBeNull();
      expect(container.querySelector('.ms-pulse-draw')).toBeNull();
      unmount();
    }
  });

  it('announces errors as alerts and everything else as status', () => {
    const { rerender } = render(<ProductState variant="error" title="Broken" />);
    expect(screen.getByRole('alert')).not.toBeNull();
    rerender(<ProductState variant="empty" title="Nothing yet" />);
    expect(screen.getByRole('status')).not.toBeNull();
  });
});

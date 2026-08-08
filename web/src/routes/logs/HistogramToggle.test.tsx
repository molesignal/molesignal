import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { HistogramToggle } from './HistogramToggle';

afterEach(() => {
  cleanup();
});

describe('HistogramToggle', () => {
  it('uses the icon button as the histogram visibility control', () => {
    const onVisibleChange = vi.fn();
    render(
      <HistogramToggle
        visible
        label="Histogram"
        onVisibleChange={onVisibleChange}
      />,
    );

    const button = screen.getByRole('button', { name: 'Histogram' });
    expect(button.getAttribute('aria-pressed')).toBe('true');
    expect(button.textContent).toBe('');

    fireEvent.click(button);

    expect(onVisibleChange).toHaveBeenCalledWith(false);
  });
});

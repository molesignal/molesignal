import { act, cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { AnimatedNumber } from '@/shell/AnimatedNumber';

const originalMatchMedia = window.matchMedia;

/** The shared test setup answers "no" to every media query; make it say what the test needs. */
function prefersReducedMotion(reduced: boolean) {
  window.matchMedia = ((query: string) => ({
    matches: reduced && query.includes('prefers-reduced-motion'),
    media: query,
    onchange: null,
    addEventListener: () => {},
    removeEventListener: () => {},
    addListener: () => {},
    removeListener: () => {},
    dispatchEvent: () => false,
  })) as typeof window.matchMedia;
}

beforeEach(() => {
  vi.useFakeTimers({
    toFake: ['requestAnimationFrame', 'cancelAnimationFrame', 'performance'],
  });
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  window.matchMedia = originalMatchMedia;
});

describe('AnimatedNumber', () => {
  it('renders the final figure at once for people who ask for less motion', () => {
    prefersReducedMotion(true);
    render(<AnimatedNumber value={12.5} decimals={1} />);
    expect(screen.getByText('12.5')).not.toBeNull();
  });

  it('counts up from zero to the figure', async () => {
    prefersReducedMotion(false);
    render(<AnimatedNumber value={12.5} decimals={1} />);
    expect(screen.getByText('0.0')).not.toBeNull();

    await act(async () => {
      vi.advanceTimersByTime(350);
    });
    const midway = Number(screen.getByText(/^\d+\.\d$/).textContent);
    expect(midway).toBeGreaterThan(0);
    expect(midway).toBeLessThan(12.5);

    await act(async () => {
      vi.advanceTimersByTime(1000);
    });
    expect(screen.getByText('12.5')).not.toBeNull();
  });

  it('keeps the requested precision', () => {
    prefersReducedMotion(true);
    render(<AnimatedNumber value={3} decimals={2} />);
    expect(screen.getByText('3.00')).not.toBeNull();
  });

  it('tweens from what is on screen when the value changes', async () => {
    prefersReducedMotion(false);
    const { rerender } = render(<AnimatedNumber value={10} />);
    await act(async () => {
      vi.advanceTimersByTime(1000);
    });
    expect(screen.getByText('10')).not.toBeNull();

    rerender(<AnimatedNumber value={20} />);
    // Still showing the old figure until the first frame, not snapping to zero.
    expect(screen.getByText('10')).not.toBeNull();
    await act(async () => {
      vi.advanceTimersByTime(1000);
    });
    expect(screen.getByText('20')).not.toBeNull();
  });

  it('shows a non-finite value as given instead of animating toward it', () => {
    prefersReducedMotion(false);
    render(<AnimatedNumber value={Number.NaN} />);
    expect(screen.getByText('NaN')).not.toBeNull();
  });
});

import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { MetricCard } from './MetricCard';

const originalMatchMedia = window.matchMedia;

// The figure counts up unless motion is reduced; these tests read the final figure.
beforeEach(() => {
  window.matchMedia = ((query: string) => ({
    matches: query.includes('prefers-reduced-motion'),
    media: query,
    onchange: null,
    addEventListener: () => {},
    removeEventListener: () => {},
    addListener: () => {},
    removeListener: () => {},
    dispatchEvent: () => false,
  })) as typeof window.matchMedia;
});

afterEach(() => {
  cleanup();
  window.matchMedia = originalMatchMedia;
});

describe('MetricCard', () => {
  it('sets the unit apart from the figure it qualifies', () => {
    render(
      <MetricCard
        label="Raw intake"
        scope="Last 24 hours"
        value="1.2 GiB"
        detail="3.1 MiB/h"
        onClick={vi.fn()}
      />,
    );

    expect(screen.getByText('1.2')).not.toBeNull();
    expect(screen.getByText('GiB')).not.toBeNull();
    expect(screen.getByText('Last 24 hours')).not.toBeNull();
    expect(screen.getByText('3.1 MiB/h')).not.toBeNull();
  });

  it('shows state in the figure itself, not in a decoration', () => {
    render(
      <MetricCard
        label="Active alerts"
        value="3"
        detail="3 firing"
        tone="red"
        onClick={vi.fn()}
      />,
    );
    expect(screen.getByText('3').parentElement?.className).toContain('text-red');
  });

  it('leaves the figure neutral when there is nothing to flag', () => {
    render(
      <MetricCard label="Active alerts" value="0" detail="All clear" onClick={vi.fn()} />,
    );
    expect(screen.getByText('0').parentElement?.className).toContain('text-tx-0');
  });

  it('keeps a placeholder dash as is', () => {
    render(<MetricCard label="Raw intake" value="—" detail="" onClick={vi.fn()} />);
    expect(screen.getByText('—')).not.toBeNull();
  });

  it('is a single button that carries the whole card', () => {
    const onClick = vi.fn();
    render(<MetricCard label="Stored" value="4 KiB" detail="" onClick={onClick} />);

    fireEvent.click(screen.getByRole('button', { name: /Stored/ }));
    expect(onClick).toHaveBeenCalledOnce();
  });

  it('swaps the content for placeholders while loading', () => {
    render(
      <MetricCard label="Stored" value="4 KiB" detail="x" loading onClick={vi.fn()} />,
    );
    expect(screen.queryByText('4')).toBeNull();
    expect(screen.getByRole('button').getAttribute('aria-busy')).toBe('true');
  });
});

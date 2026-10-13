import '@/i18n';

import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { DiffFlamebearer, Flamebearer } from '@/api/profiles';

import { Flamegraph } from './Flamegraph';

vi.mock('@/viz/timeseries/themeAdapter', () => ({
  useThemePalette: () => ({
    palette: {
      '--green': '#15803d',
      '--red': '#dc2626',
      '--surface-muted': '#f3f4f6',
    },
    version: 0,
  }),
}));

const PROFILE: Flamebearer = {
  names: ['total', 'main', 'a', 'b'],
  levels: [
    [0, 22, 0, 0],
    [0, 22, 0, 1],
    [0, 15, 15, 2, 0, 7, 7, 3],
  ],
  numTicks: 22,
  maxSelf: 15,
  units: 'nanoseconds',
};

afterEach(cleanup);

describe('Flamegraph workbench', () => {
  it('links flame, top-function selection and function details', () => {
    const onSelectedFunctionChange = vi.fn();
    render(
      <Flamegraph
        flamebearer={PROFILE}
        onSelectedFunctionChange={onSelectedFunctionChange}
      />,
    );

    expect(screen.getByText('Function details')).toBeTruthy();
    expect(screen.getByText('Width')).toBeTruthy();

    fireEvent.click(screen.getByRole('button', { name: 'Top functions' }));
    fireEvent.click(screen.getByText('b'));
    expect(onSelectedFunctionChange).toHaveBeenLastCalledWith('b');

    fireEvent.click(screen.getByRole('button', { name: 'Flame' }));
    fireEvent.click(screen.getByRole('button', { name: 'Analyze function b' }));
    expect(onSelectedFunctionChange).toHaveBeenLastCalledWith('b');
  });

  it('shows the root as the start of a path and keeps the name for functions', () => {
    const { container } = render(
      <Flamegraph
        flamebearer={{
          names: ['total', 'main'],
          levels: [
            [0, 10, 0, 0],
            [0, 10, 4, 1],
            // A function of the program that happens to be called "total".
            [0, 6, 6, 0],
          ],
          numTicks: 10,
          maxSelf: 6,
          units: 'nanoseconds',
        }}
      />,
    );

    const bars = [...container.querySelectorAll('[data-profile-view] button[aria-label]')];
    expect(bars.map((bar) => bar.textContent)).toEqual(['/', 'main', 'total']);
    expect(bars.map((bar) => bar.getAttribute('aria-label'))).toEqual([
      'Analyze all samples',
      'Analyze function main',
      'Analyze function total',
    ]);
    // The tip names a frame; a native title would say the name again, over the tip.
    expect(bars.every((bar) => !bar.hasAttribute('title'))).toBe(true);
  });

  it('draws the tip of a frame over the page, outside the view that scrolls', () => {
    const { container } = render(<Flamegraph flamebearer={PROFILE} service="checkout" />);
    const view = container.querySelector('[data-profile-view]')!;

    expect(document.querySelector('[data-flame-tooltip]')).toBeNull();

    fireEvent.mouseEnter(screen.getByRole('button', { name: 'Analyze function a' }));
    const tip = document.querySelector('[data-flame-tooltip]')!;

    // Laid out in the view, a tip taller than a shallow graph made the view scroll.
    expect(view.contains(tip)).toBe(false);
    expect(tip.parentElement).toBe(document.body);
    expect(tip.className.split(' ')).toContain('fixed');
    expect(tip.textContent).toContain('a');
    expect(tip.textContent).toContain('checkout');

    fireEvent.mouseEnter(screen.getByRole('button', { name: 'Analyze all samples' }));
    expect(document.querySelector('[data-flame-tooltip]')!.textContent).toContain('/All samples');
  });

  it('marks a function picked outside it in a comparison', () => {
    const diff: DiffFlamebearer = {
      names: ['total', 'main', 'a'],
      levels: [
        [0, 20, 0, 0, 4],
        [0, 20, 6, 1, 4],
        [0, 14, 14, 2, 4],
      ],
      numTicks: 20,
      maxSelf: 14,
      maxAbsDelta: 4,
      units: 'nanoseconds',
    };
    render(<Flamegraph flamebearer={diff} diff selectedFunction="a" />);

    expect(screen.getByRole('button', { name: 'Analyze function a' }).className).toContain('ring-indigo');
    expect(screen.getByRole('button', { name: 'Analyze function main' }).className).not.toContain(
      'ring-2 ring-indigo ring-offset-1',
    );
    // A comparison has one view: no switch to a call tree or a table, and its own legend.
    expect(screen.queryByRole('button', { name: 'Top functions' })).toBeNull();
    expect(screen.getByText('Increase')).toBeTruthy();
  });
});

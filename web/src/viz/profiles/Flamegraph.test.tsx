import '@/i18n';

import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import type { Flamebearer } from '@/api/profiles';

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
});

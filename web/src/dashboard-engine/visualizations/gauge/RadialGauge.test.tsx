import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';

import { RadialGauge, type RadialGaugeProps } from './RadialGauge';

afterEach(cleanup);

describe('RadialGauge', () => {
  it('renders arcs, labels, and one complete image semantic', () => {
    const { container } = renderGauge();

    expect(
      screen.getByRole('img', {
        name: 'CPU usage: 75%; 0%–100%',
      }),
    ).toBeTruthy();
    expect(screen.getByText('CPU usage')).toBeTruthy();
    expect(screen.getByText('75%')).toBeTruthy();
    expect(screen.getByText('80%')).toBeTruthy();
    expect(screen.getAllByTestId('gauge-threshold-interval')).toHaveLength(2);
    expect(screen.getByTestId('gauge-active-arc').getAttribute('stroke')).toBe(
      'var(--yellow)',
    );
    expect(container.querySelectorAll('[tabindex]')).toHaveLength(0);
  });

  it('can hide threshold decoration independently', () => {
    renderGauge({
      showThresholdMarkers: false,
      showThresholdLabels: false,
    });

    expect(screen.queryByTestId('gauge-threshold-interval')).toBeNull();
    expect(screen.queryByTestId('gauge-threshold-label')).toBeNull();
    expect(screen.getByTestId('gauge-active-arc')).toBeTruthy();
  });

  it('keeps the accessible name while hiding secondary compact labels', () => {
    renderGauge({ height: 100 });

    expect(
      screen.getByRole('img', {
        name: 'CPU usage: 75%; 0%–100%',
      }),
    ).toBeTruthy();
    expect(screen.queryByText('CPU usage')).toBeNull();
    expect(screen.queryByText('0%')).toBeNull();
    expect(screen.queryByText('100%')).toBeNull();
    expect(screen.queryByTestId('gauge-threshold-label')).toBeNull();
    expect(screen.getByText('75%')).toBeTruthy();
  });
});

function renderGauge(overrides: Partial<RadialGaugeProps> = {}) {
  const props: RadialGaugeProps = {
    value: 75,
    valueText: '75%',
    name: 'CPU usage',
    range: { min: 0, max: 100 },
    minimumText: '0%',
    maximumText: '100%',
    color: 'var(--yellow)',
    thresholdIntervals: [
      { start: 0, end: 80, color: 'var(--green)' },
      { start: 80, end: 100, color: 'var(--red)', label: '80%' },
    ],
    showThresholdMarkers: true,
    showThresholdLabels: true,
    height: 240,
    ...overrides,
  };
  return render(<RadialGauge {...props} />);
}

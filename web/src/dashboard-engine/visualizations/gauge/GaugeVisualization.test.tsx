import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';

import { createDashboardPanel } from '../../factories';
import type { DataField, PanelData } from '../../schema';
import {
  GaugeVisualization,
  reduceGaugeValues,
} from './GaugeVisualization';

afterEach(cleanup);

describe('GaugeVisualization', () => {
  it('reduces the latest finite value and preserves field display mapping', () => {
    renderGauge(
      numericField([10, Number.NaN, 75], {
        displayName: 'CPU usage',
        unit: 'percent',
        decimals: 0,
        min: 0,
        max: 100,
        mappings: [
          {
            type: 'range',
            from: 70,
            to: 80,
            result: { text: 'Warning', color: 'var(--yellow)' },
          },
        ],
      }),
    );

    expect(
      screen.getByRole('img', {
        name: 'CPU usage: Warning; 0%–100%',
      }),
    ).toBeTruthy();
    expect(screen.getByText('Warning')).toBeTruthy();
    expect(screen.getByTestId('gauge-active-arc').getAttribute('stroke')).toBe(
      'var(--yellow)',
    );
  });

  it('clamps an out-of-range arc without changing the displayed value', () => {
    renderGauge(
      numericField([150], {
        displayName: 'Queue depth',
        min: 0,
        max: 100,
      }),
    );

    expect(
      screen.getByRole('img', {
        name: 'Queue depth: 150; 0–100',
      }),
    ).toBeTruthy();
    expect(screen.getByText('150')).toBeTruthy();
    expect(screen.getByTestId('gauge-active-arc').getAttribute('d')).toBe(
      screen.getByTestId('gauge-track').getAttribute('d'),
    );
  });

  it('renders an empty state for non-finite numeric data', () => {
    renderGauge(numericField([Number.NaN, null]));

    expect(screen.getByText('No data')).toBeTruthy();
    expect(screen.queryByRole('img')).toBeNull();
  });

  it('supports all configured reduction modes', () => {
    const values = [2, null, 6, Number.NaN, 4];
    expect(reduceGaugeValues(values, 'last')).toBe(4);
    expect(reduceGaugeValues(values, 'min')).toBe(2);
    expect(reduceGaugeValues(values, 'max')).toBe(6);
    expect(reduceGaugeValues(values, 'mean')).toBe(4);
    expect(reduceGaugeValues(values, 'avg')).toBe(4);
    expect(reduceGaugeValues(values, 'sum')).toBe(12);
  });
});

function renderGauge(field: DataField) {
  const panel = createDashboardPanel([], 'gauge');
  return render(
    <GaugeVisualization
      panel={panel}
      data={panelData(field)}
      options={{
        calculation: 'last',
        showThresholdMarkers: true,
        showThresholdLabels: false,
      }}
      height={240}
    />,
  );
}

function panelData(field: DataField): PanelData {
  return {
    state: 'done',
    frames: [
      {
        refId: 'A',
        length: field.values.length,
        fields: [field],
      },
    ],
    timeRange: { from: 0, to: 1 },
  };
}

function numericField(
  values: unknown[],
  config: DataField['config'] = {},
): DataField {
  return {
    id: 'value',
    name: 'value',
    type: 'number',
    values,
    config,
  };
}

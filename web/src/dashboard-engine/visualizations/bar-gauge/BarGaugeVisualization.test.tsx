import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';

import { createDashboardPanel } from '../../factories';
import type { DataField, PanelData } from '../../schema';
import { BarGaugeVisualization } from './BarGaugeVisualization';
import { prepareBarGaugeValues } from './model';

afterEach(cleanup);

describe('BarGaugeVisualization', () => {
  it('builds a stable per-field range with mappings and threshold markers', () => {
    const [item] = prepareBarGaugeValues(
      panelData(
        numericField([10, 75], {
          displayName: 'CPU',
          min: 0,
          max: 100,
          thresholds: {
            mode: 'percentage',
            steps: [
              { value: null, color: 'var(--green)' },
              { value: 70, color: 'var(--red)' },
            ],
          },
          mappings: [
            {
              type: 'range',
              from: 70,
              result: { text: 'Hot', color: 'var(--yellow)' },
            },
          ],
        }),
      ).frames,
      'last',
    );

    expect(item?.range).toEqual({ min: 0, max: 100 });
    expect(item?.ratio).toBe(0.75);
    expect(item?.display.text).toBe('Hot');
    expect(item?.color).toBe('var(--yellow)');
    expect(item?.markers).toEqual([70]);
  });

  it('renders horizontal bars with native meter semantics', () => {
    renderGauge(numericField([25], { displayName: 'Queue', min: 0, max: 100 }), {
      orientation: 'horizontal',
      calculation: 'last',
      displayMode: 'basic',
    });

    const meter = screen.getByRole('meter', { name: 'Queue' });
    expect(meter.getAttribute('aria-valuemin')).toBe('0');
    expect(meter.getAttribute('aria-valuemax')).toBe('100');
    expect(meter.getAttribute('aria-valuenow')).toBe('25');
    expect(screen.getByTestId('bar-gauge-fill').getAttribute('style')).toContain(
      'width: 25%',
    );
  });

  it('uses the shared stable range when a field has no explicit bounds', () => {
    const [item] = prepareBarGaugeValues(
      panelData(numericField([20, 30])).frames,
      'last',
    );
    expect(item?.range).toEqual({ min: 0, max: 100 });
    expect(item?.ratio).toBe(0.3);
  });

  it('renders vertical threshold bars and display mappings', () => {
    renderGauge(
      numericField([80], {
        displayName: 'Memory',
        min: 0,
        max: 100,
        thresholds: {
          mode: 'absolute',
          steps: [
            { value: null, color: 'var(--green)' },
            { value: 70, color: 'var(--red)' },
          ],
        },
        mappings: [
          { type: 'value', value: 80, result: { text: 'Critical' } },
        ],
      }),
      {
        orientation: 'vertical',
        displayMode: 'thresholds',
        showThresholdMarkers: true,
      },
    );

    expect(screen.getByRole('meter', { name: 'Memory' })).toBeTruthy();
    expect(screen.getByText('Critical')).toBeTruthy();
    expect(screen.getByTestId('bar-gauge-fill').getAttribute('style')).toContain(
      'height: 80%',
    );
    expect(screen.getByTestId('bar-gauge-threshold-marker')).toBeTruthy();
  });

  it('renders the shared empty state without finite values', () => {
    renderGauge(numericField([null, Number.NaN]), {});
    expect(screen.getByText('No data')).toBeTruthy();
    expect(screen.queryByRole('meter')).toBeNull();
  });
});

function renderGauge(field: DataField, options: Record<string, unknown>) {
  return render(
    <BarGaugeVisualization
      panel={createDashboardPanel([], 'bar_gauge')}
      data={panelData(field)}
      options={options}
      height={180}
    />,
  );
}

function panelData(field: DataField): PanelData {
  return {
    state: 'done',
    frames: [{ refId: 'A', length: field.values.length, fields: [field] }],
    timeRange: { from: 0, to: 1 },
  };
}

function numericField(
  values: unknown[],
  config: DataField['config'] = {},
): DataField {
  return { id: 'value', name: 'value', type: 'number', values, config };
}

import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';

import { createDashboardPanel } from '../../factories';
import type { DataField, PanelData } from '../../schema';
import { bigValueFontSize } from './BigValue';
import { finitePercentChange, prepareStatValues } from './model';
import { sparklineGeometry } from './Sparkline';
import { StatVisualization } from './StatVisualization';

afterEach(cleanup);

describe('StatVisualization', () => {
  it('reduces finite values, preserves mappings, and calculates change', () => {
    const field = numericField([10, Number.NaN, 15], {
      displayName: 'Request rate',
      mappings: [
        {
          type: 'value',
          value: 15,
          result: { text: 'Healthy', color: 'var(--green)' },
        },
      ],
    });
    const [value] = prepareStatValues(panelData(field).frames, 'last');

    expect(value?.display.text).toBe('Healthy');
    expect(value?.color).toBe('var(--green)');
    expect(value?.sparkline).toEqual([10, 15]);
    expect(value?.percentChange).toBe(50);
    expect(finitePercentChange([0, 10])).toBeNull();
  });

  it('renders a responsive accessible BigValue with a sparkline', () => {
    renderStat(numericField([10, 12, 15], { displayName: 'Request rate' }), {
      calculation: 'last',
      textMode: 'value_and_name',
      graphMode: 'area',
      colorMode: 'value',
      showPercentChange: true,
    });

    expect(
      screen.getByRole('img', { name: 'Request rate: 15; +50.0%' }),
    ).toBeTruthy();
    expect(screen.getByText('Request rate')).toBeTruthy();
    expect(screen.getByText('+50.0%')).toBeTruthy();
    expect(document.querySelector('svg')).toBeTruthy();
  });

  it('supports name-only and compact layouts without non-finite geometry', () => {
    renderStat(numericField([5], { displayName: 'Queue' }), {
      textMode: 'name',
      graphMode: 'area',
      colorMode: 'none',
    });

    expect(screen.getByText('Queue')).toBeTruthy();
    expect(screen.queryByText('5')).toBeNull();
    expect(bigValueFontSize('123456789', 80, 30)).toBeGreaterThanOrEqual(18);
    expect(sparklineGeometry([4, 4])).toEqual({
      line: 'M 0,16 L 100,16',
      area: 'M 0,32 L 0,16 L 100,16 L 100,32 Z',
    });
    expect(sparklineGeometry([Number.NaN])).toBeNull();
  });

  it('renders the shared empty state for non-finite samples', () => {
    renderStat(numericField([Number.NaN, null]), {});
    expect(screen.getByText('No data')).toBeTruthy();
  });
});

function renderStat(field: DataField, options: Record<string, unknown>) {
  return render(
    <StatVisualization
      panel={createDashboardPanel([], 'stat')}
      data={panelData(field)}
      options={options}
      height={120}
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

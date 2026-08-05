import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';

import { createDashboardPanel } from '../../factories';
import type { DataField, DataFrame, PanelData } from '../../schema';
import { HeatmapVisualization } from './HeatmapVisualization';
import {
  aggregateFiniteWindows,
  heatmapIntensity,
  prepareHeatmap,
} from './model';

afterEach(cleanup);

describe('HeatmapVisualization', () => {
  it('prepares aligned numeric rows against one global range', () => {
    const model = prepareHeatmap([
      frame([
        field('time', 'time', [1_700_000_000, 1_700_000_010, 1_700_000_020]),
        field('cpu', 'number', [1, 2, 3]),
        field('memory', 'number', [null, 8, 10]),
      ]),
    ]);

    expect(model?.rows.map((row) => row.name)).toEqual(['cpu', 'memory']);
    expect(model?.rows[1]?.values).toEqual([null, 8, 10]);
    expect(model?.min).toBe(1);
    expect(model?.max).toBe(10);
    expect(model?.columns).toBe(3);
    expect(model?.firstColumnLabel).not.toBe('1');
  });

  it('aggregates consecutive windows to at most 120 columns', () => {
    const samples = Array.from({ length: 240 }, (_, index) => index);
    samples[2] = Number.NaN;
    const model = prepareHeatmap([frame([field('value', 'number', samples)])]);

    expect(model?.columns).toBe(120);
    expect(model?.windowSize).toBe(2);
    expect(model?.rows[0]?.values[0]).toBe(0.5);
    expect(model?.rows[0]?.values[1]).toBe(3);
    expect(aggregateFiniteWindows([null, Number.NaN, 4, 8], 4, 2)).toEqual([
      null,
      6,
    ]);
  });

  it('uses a stable medium intensity for constant values', () => {
    const model = prepareHeatmap([
      frame([field('value', 'number', [7, 7, 7])]),
    ])!;
    expect(model.constant).toBe(true);
    expect(model.min).toBe(7);
    expect(model.max).toBe(7);
    expect(heatmapIntensity(7, model)).toBe(0.56);
    expect(heatmapIntensity(null, model)).toBe(0);
  });

  it('renders token-colored cells and an accessible matrix summary', () => {
    renderHeatmap(
      frame([
        field('cpu', 'number', [1, null, 3]),
        field('memory', 'number', [4, 5, 6]),
      ]),
      { colorScheme: 'greens' },
    );

    expect(
      screen.getByRole('img', {
        name: 'Heatmap with 2 series and 3 columns; values 1 to 6',
      }),
    ).toBeTruthy();
    const cells = screen.getAllByTestId('heatmap-cell');
    expect(cells).toHaveLength(6);
    expect(cells[0]?.getAttribute('style')).toContain(
      'background-color: var(--green)',
    );
    expect(cells[1]?.getAttribute('title')).toContain('No value');
  });

  it('renders the shared empty state when every sample is empty', () => {
    renderHeatmap(frame([field('value', 'number', [null, Number.NaN])]), {});
    expect(screen.getByText('No data')).toBeTruthy();
    expect(screen.queryByRole('img')).toBeNull();
  });
});

function renderHeatmap(value: DataFrame, options: Record<string, unknown>) {
  const data: PanelData = {
    state: 'done',
    frames: [value],
    timeRange: { from: 0, to: 1 },
  };
  return render(
    <HeatmapVisualization
      panel={createDashboardPanel([], 'heatmap')}
      data={data}
      options={options}
      height={180}
    />,
  );
}

function frame(fields: DataField[]): DataFrame {
  return {
    refId: 'A',
    length: Math.max(0, ...fields.map((item) => item.values.length)),
    fields,
  };
}

function field(id: string, type: DataField['type'], values: unknown[]): DataField {
  return { id, name: id, type, values };
}

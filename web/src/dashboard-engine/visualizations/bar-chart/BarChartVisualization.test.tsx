import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';

import { createDashboardPanel } from '../../factories';
import type { DataField, DataFrame, PanelData } from '../../schema';
import { BarChartVisualization } from './BarChartVisualization';
import { buildBarChartGeometry } from './geometry';
import { prepareBarChart } from './model';

afterEach(cleanup);

describe('BarChartVisualization', () => {
  it('prepares tabular categories, grouped series, colors, and a zero-inclusive range', () => {
    const model = prepareBarChart([
      frame([
        field('service', 'string', ['api', 'web']),
        field('latency', 'number', [12, -4]),
        field('errors', 'number', [2, 5]),
      ]),
    ]);

    expect(model?.categories.map((category) => category.label)).toEqual(['api', 'web']);
    expect(model?.series.map((series) => series.name)).toEqual(['latency', 'errors']);
    expect(new Set(model?.series.map((series) => series.color)).size).toBe(2);
    expect(model?.range).toEqual({ min: -4, max: 12 });
  });

  it('falls back to one reduced category per numeric field', () => {
    const model = prepareBarChart([
      frame([
        field('cpu', 'number', [10, 20]),
        field('memory', 'number', [30, 40]),
      ]),
    ]);

    expect(model?.categories.map((category) => category.label)).toEqual(['cpu', 'memory']);
    expect(model?.categories.map((category) => category.values.value?.value)).toEqual([20, 40]);
    expect(model?.series).toHaveLength(1);
  });

  it('keeps the most recent 120 categories', () => {
    const labels = Array.from({ length: 125 }, (_, index) => `category-${index}`);
    const model = prepareBarChart([
      frame([
        field('category', 'string', labels),
        field('value', 'number', labels.map((_, index) => index)),
      ]),
    ]);

    expect(model?.truncated).toBe(true);
    expect(model?.categories).toHaveLength(120);
    expect(model?.categories[0]?.label).toBe('category-5');
  });

  it('allocates scrollable plot width for the bounded category set', () => {
    const labels = Array.from({ length: 125 }, (_, index) => `category-${index}`);
    const data = panelData(
      frame([
        field('category', 'string', labels),
        field('value', 'number', labels.map((_, index) => index)),
      ]),
    );
    const { container } = render(
      <BarChartVisualization
        panel={createDashboardPanel([], 'bar_chart')}
        data={data}
        options={{ orientation: 'vertical' }}
        height={220}
      />,
    );

    expect(container.firstElementChild?.className).toContain('overflow-auto');
    expect(container.querySelector('svg')?.getAttribute('style')).toContain(
      'width: 2224px',
    );
  });

  it('builds finite vertical and horizontal geometry around zero', () => {
    const model = prepareBarChart([
      frame([
        field('category', 'string', ['loss', 'gain']),
        field('value', 'number', [-5, 10]),
      ]),
    ])!;
    const vertical = buildBarChartGeometry(model, 480, 220, 'vertical', 0.7, 'always');
    const horizontal = buildBarChartGeometry(model, 480, 220, 'horizontal', 0.7, 'always');

    expect(vertical.rects).toHaveLength(2);
    expect(horizontal.rects).toHaveLength(2);
    expect(vertical.rects.every((rect) => rect.height >= 1)).toBe(true);
    expect(horizontal.rects.every((rect) => rect.width >= 1)).toBe(true);
    expect(vertical.showValues).toBe(true);
    expect(horizontal.zeroLine.x1).toBe(horizontal.zeroLine.x2);
  });

  it('renders an accessible SVG with bar titles in both orientations', () => {
    const data = panelData(
      frame([
        field('service', 'string', ['api', 'web']),
        field('requests', 'number', [20, 35]),
      ]),
    );
    const { rerender } = render(
      <BarChartVisualization
        panel={createDashboardPanel([], 'bar_chart')}
        data={data}
        options={{ orientation: 'vertical', showValues: 'always' }}
        height={220}
      />,
    );
    expect(
      screen.getByRole('img', { name: 'Bar chart with 2 categories and 1 series' }),
    ).toBeTruthy();
    expect(screen.getAllByTestId('bar-chart-bar')).toHaveLength(2);
    expect(screen.getByText('api · requests: 20')).toBeTruthy();

    rerender(
      <BarChartVisualization
        panel={createDashboardPanel([], 'bar_chart')}
        data={data}
        options={{ orientation: 'horizontal', showValues: 'never' }}
        height={220}
      />,
    );
    expect(screen.getAllByTestId('bar-chart-bar')).toHaveLength(2);
  });
});

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

function panelData(value: DataFrame): PanelData {
  return {
    state: 'done',
    frames: [value],
    timeRange: { from: 0, to: 1 },
  };
}

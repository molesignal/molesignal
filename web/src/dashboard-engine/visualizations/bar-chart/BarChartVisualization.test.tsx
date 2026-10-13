import { cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it } from 'vitest';

import { TooltipProvider } from '@/shell/ui/tooltip';

import { createDashboardPanel } from '../../factories';
import type { DataField, DataFrame, PanelData } from '../../schema';
import { BarChartVisualization } from './BarChartVisualization';
import { buildBarChartGeometry, valueScale } from './geometry';
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

  it('reads the values against round marks that no bar runs past', () => {
    expect(valueScale({ range: { min: 0, max: 350 } })).toEqual({ min: 0, max: 400, ticks: [0, 100, 200, 300, 400] });
    expect(valueScale({ range: { min: -4, max: 12 } })).toEqual({ min: -5, max: 15, ticks: [-5, 0, 5, 10, 15] });
    expect(valueScale({ range: { min: 0, max: 1 } }).ticks).toEqual([0, 0.25, 0.5, 0.75, 1]);
    // A step of a tenth is counted in steps, and comes out round.
    expect(valueScale({ range: { min: 0, max: 0.3 } }).ticks).toEqual([0, 0.1, 0.2, 0.3]);
  });

  it('keeps a few bars in a wide panel from becoming slabs, and marks the axis across the plot', () => {
    const model = prepareBarChart([
      frame([
        field('level', 'string', ['info', 'warn', 'error']),
        field('count', 'number', [350, 100, 50]),
      ]),
    ])!;
    const wide = buildBarChartGeometry(model, 1_600, 640, 'vertical', 0.7, 'auto');

    expect(wide.rects.every((rect) => rect.width <= 56)).toBe(true);
    // The tallest bar stops short of the top line, which is the last mark of the scale.
    expect(Math.min(...wide.rects.map((rect) => rect.y))).toBeGreaterThan(Math.min(...wide.gridLines.map((line) => line.y1)));
    expect(wide.valueTicks.map((tick) => tick.text)).toEqual(['0', '100', '200', '300', '400']);
    expect(wide.gridLines).toHaveLength(4);
    expect(wide.showValues).toBe(true);

    const lying = buildBarChartGeometry(model, 1_600, 640, 'horizontal', 0.7, 'auto');
    expect(lying.rects.every((rect) => rect.height <= 56)).toBe(true);
    expect(lying.gridLines.every((line) => line.x1 === line.x2)).toBe(true);
  });

  it('cuts a category label to the room its bar has', () => {
    const model = prepareBarChart([
      frame([
        field('service', 'string', ['payment-gateway-production', 'checkout']),
        field('count', 'number', [3, 2]),
      ]),
    ])!;

    expect(buildBarChartGeometry(model, 1_200, 240, 'vertical', 0.7, 'auto').categoryLabels[0]?.text).toBe(
      'payment-gateway-production',
    );
    // Seventy pixels of a category hold ten characters: nine of the name and the mark that it goes on.
    expect(buildBarChartGeometry(model, 200, 240, 'vertical', 0.7, 'auto').categoryLabels[0]?.text).toBe('payment-g…');
  });

  it('renders accessible bars with hover details in both orientations', async () => {
    const user = userEvent.setup();
    const data = panelData(
      frame([
        field('service', 'string', ['api', 'web']),
        field('requests', 'number', [20, 35]),
      ]),
    );
    const { rerender } = render(
      <TooltipProvider delayDuration={0}>
        <BarChartVisualization
          panel={createDashboardPanel([], 'bar_chart')}
          data={data}
          options={{ orientation: 'vertical', showValues: 'always' }}
          height={220}
        />
      </TooltipProvider>,
    );
    expect(
      screen.getByRole('img', { name: 'Bar chart with 2 categories and 1 series' }),
    ).toBeTruthy();
    const bars = screen.getAllByTestId('bar-chart-bar');
    expect(bars).toHaveLength(2);
    expect(bars[0]?.getAttribute('aria-label')).toBe('api · requests: 20');
    await user.hover(bars[0]!);
    const tooltip = await screen.findByRole('tooltip');
    expect(tooltip).toBeTruthy();
    expect(within(tooltip).getByText('api')).toBeTruthy();
    expect(within(tooltip).getByText('requests: 20')).toBeTruthy();

    rerender(
      <TooltipProvider delayDuration={0}>
        <BarChartVisualization
          panel={createDashboardPanel([], 'bar_chart')}
          data={data}
          options={{ orientation: 'horizontal', showValues: 'never' }}
          height={220}
        />
      </TooltipProvider>,
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

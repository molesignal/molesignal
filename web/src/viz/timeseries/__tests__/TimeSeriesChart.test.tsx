import {
  cleanup,
  fireEvent,
  render,
  screen,
} from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';

import {
  TimeSeriesChart,
  TimeSeriesSparkline,
} from '@/viz/timeseries/TimeSeriesChart';

afterEach(cleanup);

describe('TimeSeriesChart', () => {
  it('uses the unified Canvas renderer surface and never emits an SVG data plot', () => {
    const view = render(
      <TimeSeriesChart
        ariaLabel="Request rate"
        series={[
          {
            id: 'requests',
            name: 'requests',
            timestamps: [1, 2, 3],
            data: [2, 4, 3],
          },
        ]}
      />,
    );

    expect(screen.getByTestId('time-series-chart').getAttribute('data-renderer')).toBe(
      'uplot-canvas',
    );
    expect(screen.getByRole('img', { name: 'Request rate' })).not.toBeNull();
    expect(view.container.querySelector('svg')).toBeNull();
  });

  it('renders compact trends through the same component', () => {
    render(
      <TimeSeriesSparkline
        data={[1, null, 3]}
        ariaLabel="Error trend"
        color="var(--red)"
      />,
    );

    const chart = screen.getByTestId('time-series-chart');
    expect(chart.getAttribute('data-renderer')).toBe('uplot-canvas');
    expect(screen.queryByRole('list', { name: 'Series legend' })).toBeNull();
  });

  it('keeps empty state distinct from a zero-valued series', () => {
    const { rerender } = render(
      <TimeSeriesChart series={[{ name: 'zero', data: [0, 0, 0] }]} />,
    );
    expect(screen.queryByText('No data in this time range')).toBeNull();

    rerender(<TimeSeriesChart series={[{ name: 'missing', data: [null, null] }]} />);
    expect(screen.getByText('No data in this time range')).not.toBeNull();
  });

  it('applies percent stacking as a fixed percentage scale', () => {
    render(
      <TimeSeriesChart
        series={[
          { id: 'success', name: 'Success', data: [1, 3] },
          { id: 'error', name: 'Error', data: [1, 1] },
        ]}
        options={{ stackMode: 'percent' }}
      />,
    );

    expect(
      screen
        .getByTestId('time-series-chart')
        .getAttribute('data-stack-mode'),
    ).toBe('percent');
  });

  it('supports a compact legend without changing the default density', () => {
    render(
      <TimeSeriesChart
        showLegend
        legendDensity="compact"
        series={[
          {
            id: 'p95',
            name: 'P95 latency',
            timestamps: [1, 2],
            data: [100, 200],
          },
        ]}
      />,
    );

    expect(
      screen.getByRole('table', { name: 'Series legend' }).className,
    ).toContain('type-micro');
    expect(
      screen.getByRole('columnheader', { name: 'Name' }).className,
    ).toContain('py-0');
  });

  it('wraps long table legend names and vertically centers their series marker', () => {
    const longName =
      'cache_hits_total{service.instance.id="3HM6t30kxBVzcFYX0QXUd4IhxQi",service.name="molesignal"}';
    render(
      <TimeSeriesChart
        series={[{ id: 'cache-hits', name: longName, data: [0, 0] }]}
        options={{ legendMode: 'table', legendPlacement: 'bottom' }}
      />,
    );

    const label = screen.getByRole('button', { name: longName });
    expect(label.className).toContain('whitespace-normal');
    expect(label.className).toContain('[overflow-wrap:anywhere]');

    const iconCell = label.closest('tr')?.querySelector('td:first-child');
    expect(iconCell?.className).toContain('items-center');
    expect(iconCell?.className).toContain('self-stretch');
  });

  it('renders an expandable metric identity instead of the raw label set', () => {
    render(
      <TimeSeriesChart
        height="auto"
        series={[
          {
            id: 'cache-misses',
            metricName: 'cache_misses_total',
            name: 'cache_misses_total{service.name="molesignal"}',
            labels: {
              'deployment.environment.name': 'production',
              'service.name': 'molesignal',
            },
            data: [0, 0],
          },
        ]}
        options={{
          legendMode: 'table',
          legendPlacement: 'bottom',
          legendStats: ['last'],
        }}
        seriesIdentity={{
          title: 'Series',
          countLabel: '1 series',
          nameLabel: 'Name',
          labelCountLabel: (count) => `${count} labels`,
          expandLabel: (metricName) => `Show labels for ${metricName}`,
          collapseLabel: (metricName) => `Hide labels for ${metricName}`,
          statLabels: { last: 'Last' },
        }}
      />,
    );

    expect(screen.getByTestId('time-series-legend-heading').textContent)
      .toContain('1 series');
    expect(
      screen.getByRole('button', { name: 'cache_misses_total' }).textContent,
    ).toBe('cache_misses_total');
    expect(screen.getByText('2 labels')).not.toBeNull();
    const chart = screen.getByTestId('time-series-chart');
    const legend = screen.getByTestId('time-series-legend');
    expect(chart.style.height).toBe('auto');
    expect(chart.className).toContain('overflow-visible');
    expect(screen.getByTestId('time-series-content').className).toContain(
      'flex-none',
    );
    expect(screen.getByTestId('time-series-plot').className).toContain(
      'h-[clamp(320px,42vh,460px)]',
    );
    expect(legend.getAttribute('data-adaptive-height')).toBe('true');
    expect(legend.className).toContain('overflow-visible');
    expect(legend.className).not.toContain('max-h-[42%]');

    fireEvent.click(screen.getByText('2 labels'));
    expect(screen.getByTestId('series-identifier-labels')).not.toBeNull();
  });

  it('renders List, Table, and Hidden modes with Grafana legend placement', () => {
    const series = [
      { id: 'requests', name: 'Requests', data: [1, 2, 3] },
    ];
    const { rerender } = render(
      <TimeSeriesChart
        series={series}
        options={{ legendMode: 'list', legendPlacement: 'bottom' }}
      />,
    );

    expect(screen.getByRole('list', { name: 'Series legend' })).toBeTruthy();
    expect(screen.queryByText(/Last:/)).toBeNull();
    expect(
      screen
        .getByTestId('time-series-legend')
        .getAttribute('data-legend-placement'),
    ).toBe('bottom');

    rerender(
      <TimeSeriesChart
        series={series}
        options={{ legendMode: 'table', legendPlacement: 'right' }}
      />,
    );
    expect(screen.getByRole('table', { name: 'Series legend' })).toBeTruthy();
    expect(screen.getByTestId('time-series-content').className).toContain(
      'flex-row',
    );
    expect(
      screen
        .getByTestId('time-series-legend')
        .getAttribute('data-legend-placement'),
    ).toBe('right');

    rerender(
      <TimeSeriesChart
        series={series}
        options={{ legendMode: 'hidden', legendPlacement: 'right' }}
      />,
    );
    expect(screen.queryByTestId('time-series-legend')).toBeNull();
  });

  it('uses Grafana legend isolation and modifier-toggle behavior', () => {
    render(
      <TimeSeriesChart
        series={[
          { id: 'a', name: 'Alpha', data: [1, 2] },
          { id: 'b', name: 'Beta', data: [2, 3] },
          { id: 'c', name: 'Gamma', data: [3, 4] },
        ]}
      />,
    );

    const alpha = screen.getByRole('button', { name: 'Alpha' });
    const beta = screen.getByRole('button', { name: 'Beta' });
    const gamma = screen.getByRole('button', { name: 'Gamma' });

    fireEvent.click(beta);
    expect(alpha.getAttribute('aria-pressed')).toBe('false');
    expect(beta.getAttribute('aria-pressed')).toBe('true');
    expect(gamma.getAttribute('aria-pressed')).toBe('false');

    fireEvent.click(beta);
    expect(alpha.getAttribute('aria-pressed')).toBe('true');
    expect(beta.getAttribute('aria-pressed')).toBe('true');
    expect(gamma.getAttribute('aria-pressed')).toBe('true');

    fireEvent.click(gamma, { ctrlKey: true });
    expect(gamma.getAttribute('aria-pressed')).toBe('false');
    expect(alpha.getAttribute('aria-pressed')).toBe('true');
  });

  it('treats height as the complete visualization and caps the bottom legend', () => {
    render(
      <TimeSeriesChart
        height={240}
        series={[
          { id: 'requests', name: 'Requests', data: [1, 2, 3] },
        ]}
      />,
    );

    expect(screen.getByTestId('time-series-chart').style.height).toBe('240px');
    expect(screen.getByTestId('time-series-plot').className).toContain('flex-1');
    expect(screen.getByTestId('time-series-legend').className).toContain(
      'max-h-[35%]',
    );
  });
});

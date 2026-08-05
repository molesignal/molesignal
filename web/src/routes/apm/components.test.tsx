import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { TrendPoint } from '@/api/apm';
import i18n from '@/i18n';

import {
  ApmPageFrame,
  averageThroughput,
  RedKpis,
  TraceIdLink,
  TrendStrip,
} from './components';

const timeSeriesChart = vi.hoisted(() => vi.fn());

vi.mock('@/viz/timeseries/TimeSeriesChart', () => ({
  TimeSeriesChart: (props: Record<string, unknown>) => {
    timeSeriesChart(props);
    return <div data-testid="request-volume-chart" />;
  },
}));

vi.mock('./Layout', () => ({
  ApmNavigation: () => <nav data-testid="apm-navigation" />,
}));

const RANGE = {
  from: Date.UTC(2026, 6, 30, 12) * 1_000,
  to: Date.UTC(2026, 6, 30, 13) * 1_000,
};

const POINTS: TrendPoint[] = [
  trendPoint(RANGE.from + 60_000_000, 24),
  trendPoint(RANGE.from + 120_000_000, 48),
  trendPoint(RANGE.from + 180_000_000, 12),
];

beforeEach(async () => {
  timeSeriesChart.mockClear();
  await i18n.changeLanguage('en-us');
});

afterEach(cleanup);

describe('TrendStrip', () => {
  it('renders bucket-normalized throughput on the selected time domain', () => {
    render(<TrendStrip points={POINTS} range={RANGE} resolution="minute" />);

    expect(screen.getByTestId('request-volume-chart')).toBeTruthy();
    expect(timeSeriesChart.mock.calls[0]?.[0]).toEqual(
      expect.objectContaining({
        xDomain: [RANGE.from, RANGE.to],
        ariaLabel: 'Request throughput trend',
        series: [
          expect.objectContaining({
            data: [0.4, 0.8, 0.2],
            timestamps: POINTS.map((point) => point.bucket_at),
          }),
        ],
        options: expect.objectContaining({
          drawStyle: 'bar',
          compactAxes: true,
          leftAxis: expect.objectContaining({
            min: 0,
            label: 'Request throughput',
            unit: 'req/s',
          }),
        }),
      }),
    );
  });

  it('switches to latency percentiles without changing the time domain', () => {
    render(<TrendStrip points={POINTS} range={RANGE} resolution="minute" />);

    fireEvent.click(screen.getByRole('button', { name: 'Latency' }));

    expect(timeSeriesChart.mock.calls.at(-1)?.[0]).toEqual(
      expect.objectContaining({
        xDomain: [RANGE.from, RANGE.to],
        ariaLabel: 'Request latency trend',
        showLegend: true,
        legendDensity: 'compact',
        series: [
          expect.objectContaining({ name: 'P50 latency', data: [100_000, 100_000, 100_000] }),
          expect.objectContaining({ name: 'P95 latency', data: [250_000, 250_000, 250_000] }),
          expect.objectContaining({ name: 'P99 latency', data: [500_000, 500_000, 500_000] }),
        ],
        options: expect.objectContaining({
          drawStyle: 'line',
          tooltipMode: 'all',
        }),
      }),
    );
  });

  it('keeps the empty state instead of mounting an empty chart', () => {
    render(<TrendStrip points={[]} range={RANGE} resolution="minute" />);

    expect(screen.getByText('No trend buckets')).toBeTruthy();
    expect(timeSeriesChart).not.toHaveBeenCalled();
  });
});

describe('RedKpis', () => {
  it('uses observed buckets for average throughput and keeps request count as context', () => {
    const red = {
      ...POINTS[0]!.red,
      request_count: 84,
      p95_micros: 250_000,
      p99_micros: 500_000,
    };

    render(<RedKpis red={red} trend={POINTS} resolution="minute" />);

    expect(screen.getByText('Request throughput')).toBeTruthy();
    expect(screen.getByText('0.47 req/s')).toBeTruthy();
    expect(screen.getByText('84 total requests')).toBeTruthy();
    expect(screen.getByText('P99 latency')).toBeTruthy();
    expect(averageThroughput(84, POINTS, 'minute')).toBeCloseTo(84 / 180);
  });
});

describe('ApmPageFrame', () => {
  it('shows the global APM navigation on top-level pages', () => {
    render(
      <MemoryRouter>
        <ApmPageFrame title="APM overview">Content</ApmPageFrame>
      </MemoryRouter>,
    );

    expect(screen.getByTestId('apm-navigation')).toBeTruthy();
  });

  it('replaces the global navigation with the service navigation on detail pages', () => {
    render(
      <MemoryRouter>
        <ApmPageFrame
          title="payment-service"
          navigation={<nav data-testid="service-navigation" />}
        >
          Content
        </ApmPageFrame>
      </MemoryRouter>,
    );

    expect(screen.getByTestId('service-navigation')).toBeTruthy();
    expect(screen.queryByTestId('apm-navigation')).toBeNull();
  });
});

describe('TraceIdLink', () => {
  it('opens the trace detail and selects the exemplar span', () => {
    render(
      <MemoryRouter>
        <TraceIdLink traceId="trace/checkout" spanId="span one" />
      </MemoryRouter>,
    );

    expect(
      screen
        .getByRole('link', {
          name: 'Open Trace: trace/checkout',
        })
        .getAttribute('href'),
    ).toBe('/traces/trace%2Fcheckout?spanId=span+one');
  });
});

function trendPoint(bucketAt: number, requestCount: number): TrendPoint {
  return {
    bucket_at: bucketAt,
    red: {
      request_count: requestCount,
      error_count: 0,
      error_rate: 0,
      duration_sum_micros: 0,
      p50_micros: 100_000,
      p95_micros: 250_000,
      p99_micros: 500_000,
      latency_partial: false,
      exemplars: [],
    },
  };
}

import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { HomeOverview } from '@/api/home';
import i18n from '@/i18n';

import { SystemHealthSection } from './SystemHealthSection';

vi.mock('@/viz/timeseries/TimeSeriesChart', () => ({
  TimeSeriesChart: ({
    series,
    options,
  }: {
    series: Array<{
      color?: string;
      data: Array<number | null>;
      timestamps?: number[];
    }>;
    options?: { tooltipMode?: string };
  }) => (
    <div
      data-testid="time-series-chart"
      data-color={series[0]?.color}
      data-values={JSON.stringify(series[0]?.data)}
      data-timestamps={JSON.stringify(series[0]?.timestamps)}
      data-tooltip-mode={options?.tooltipMode}
    />
  ),
}));

const EMPTY_OVERVIEW: HomeOverview = {
  generated_at_micros: 200,
  window: {
    start_micros: 100,
    end_micros: 200,
    window_secs: 100,
  },
  intake_status: 'no_data',
  probe_reason: null,
  intake_bytes: 0,
  stored_bytes: 0,
  rows: 0,
  compression_savings_ratio: null,
  active_streams: 0,
  total_streams: 1,
  attention_streams: 1,
  last_received_at_micros: null,
  stats_probe: { succeeded: 1, total: 1 },
  buckets: [],
  signals: [],
  streams: [
    {
      id: 'stream-1',
      name: 'empty-stream',
      stream_type: 'logs',
      status: 'no_data',
      rows: 0,
      stored_bytes: 0,
      first_received_at_micros: null,
      last_received_at_micros: null,
    },
  ],
};

afterEach(cleanup);

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
});

describe('SystemHealthSection', () => {
  it('keeps the chart frame without drawing a series when the time window is empty', () => {
    render(
      <SystemHealthSection
        overview={EMPTY_OVERVIEW}
        runtimeOverview={undefined}
        state={null}
        error={null}
        metric="intake"
        onMetricChange={vi.fn()}
        windowLabel="Last 24 hours"
        onOpenStreams={vi.fn()}
      />,
    );

    const chart = screen.getByTestId('time-series-chart');
    expect(chart.getAttribute('data-values')).toBe('[0,0]');
    expect(chart.getAttribute('data-timestamps')).toBe('[100,200]');
    expect(chart.getAttribute('data-color')).toBe('transparent');
    expect(chart.getAttribute('data-tooltip-mode')).toBe('hidden');
    expect(screen.queryByText('No data in the selected period')).toBeNull();
  });
});

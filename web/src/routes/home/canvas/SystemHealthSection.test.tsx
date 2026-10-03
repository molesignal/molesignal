import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { HomeOverview } from '@/api/home';
import type { StreamRuntime, StreamRuntimeOverview } from '@/api/streams';
import i18n from '@/i18n';

import { SystemHealthSection } from './SystemHealthSection';

vi.mock('@/viz/timeseries/TimeSeriesChart', () => ({
  TimeSeriesChart: ({
    series,
  }: {
    series: Array<{
      color?: string;
      data: Array<number | null>;
      timestamps?: number[];
    }>;
  }) => (
    <div
      data-testid="time-series-chart"
      data-color={series[0]?.color}
      data-values={JSON.stringify(series[0]?.data)}
      data-timestamps={JSON.stringify(series[0]?.timestamps)}
    />
  ),
}));

const NOW = 1_800_000_000_000_000;
const DAY = 86_400_000_000;

const EMPTY_OVERVIEW: HomeOverview = {
  generated_at_micros: 200,
  window: { start_micros: 100, end_micros: 200, window_secs: 100 },
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

const DATA_OVERVIEW: HomeOverview = {
  ...EMPTY_OVERVIEW,
  intake_bytes: 30,
  rows: 5,
  buckets: [
    { start_micros: 100, end_micros: 150, intake_bytes: 10, stored_bytes: 5, rows: 2 },
    { start_micros: 150, end_micros: 200, intake_bytes: 20, stored_bytes: 8, rows: 3 },
  ],
};

function runtimeStream(
  overrides: Partial<StreamRuntime> & Pick<StreamRuntime, 'id' | 'stream_type' | 'status'>,
): StreamRuntime {
  return {
    name: overrides.id,
    rows: 0,
    stored_bytes: 0,
    current_stored_bytes: 0,
    first_received_at_micros: null,
    last_received_at_micros: null,
    stats_available: true,
    buckets: [],
    ...overrides,
  };
}

function runtimeOverview(streams: StreamRuntime[]): StreamRuntimeOverview {
  return {
    generated_at_micros: NOW,
    window_start_micros: NOW - DAY,
    window_end_micros: NOW,
    window_secs: 86_400,
    streams,
  };
}

function renderSection(
  props: Partial<React.ComponentProps<typeof SystemHealthSection>> = {},
) {
  const callbacks = {
    onConnect: vi.fn(),
    onInvestigate: vi.fn(),
    onOpenStreams: vi.fn(),
  };
  render(
    <SystemHealthSection
      overview={EMPTY_OVERVIEW}
      runtimeOverview={undefined}
      state={null}
      error={null}
      metric="intake"
      onMetricChange={vi.fn()}
      windowLabel="Last 24 hours"
      {...callbacks}
      {...props}
    />,
  );
  return callbacks;
}

afterEach(cleanup);

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
});

describe('SystemHealthSection', () => {
  it('replaces the chart with a statement and a next step when the window is empty', () => {
    const callbacks = renderSection();

    // No chart means no axis scaled to a range nobody measured.
    expect(screen.queryByTestId('time-series-chart')).toBeNull();
    expect(screen.getByText('No data in the selected period')).not.toBeNull();
    expect(screen.getByText(/A source may have stopped sending/)).not.toBeNull();

    fireEvent.click(screen.getByRole('button', { name: 'Check data intake' }));
    expect(callbacks.onInvestigate).toHaveBeenCalledOnce();
  });

  it('draws the series when the window has data and names the window it covers', () => {
    renderSection({ overview: DATA_OVERVIEW });

    const chart = screen.getByTestId('time-series-chart');
    expect(chart.getAttribute('data-color')).toBe('var(--chart-1)');
    expect(chart.getAttribute('data-values')).toBe('[10,20]');
    expect(chart.getAttribute('data-timestamps')).toBe('[125,175]');
    expect(screen.queryByText('No data in the selected period')).toBeNull();
    expect(screen.getByText('Total · Last 24 hours')).not.toBeNull();
  });

  it('offers to connect a source when no streams are registered', () => {
    const callbacks = renderSection({ state: 'empty' });

    expect(screen.getByText('No observable streams registered yet.')).not.toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Connect a data source' }));
    expect(callbacks.onConnect).toHaveBeenCalledOnce();
  });

  it('survives a backend that answers with empty bodies', () => {
    // Proxies, mocks and older backends send `{}`; the page must degrade, not crash.
    const callbacks = renderSection({
      overview: {} as HomeOverview,
      runtimeOverview: {} as StreamRuntimeOverview,
      state: 'empty',
    });

    expect(screen.getByText('No observable streams registered yet.')).not.toBeNull();
    expect(callbacks.onConnect).not.toHaveBeenCalled();
  });

  it('shows a placeholder, not a spinner, while loading', () => {
    renderSection({ state: 'loading' });
    expect(screen.getByRole('status').getAttribute('aria-busy')).toBe('true');
  });

  it('names signals with the product labels and the same condition words as the streams table', () => {
    renderSection({
      overview: DATA_OVERVIEW,
      runtimeOverview: runtimeOverview([
        runtimeStream({
          id: 'a',
          stream_type: 'logs',
          status: 'interrupted',
          last_received_at_micros: NOW - 6 * DAY,
        }),
        runtimeStream({
          id: 'b',
          stream_type: 'metrics',
          status: 'healthy',
          rows: 100,
          last_received_at_micros: NOW - 1_000_000,
        }),
      ]),
    });

    expect(screen.getByText('Logs')).not.toBeNull();
    expect(screen.getByText('Metrics')).not.toBeNull();
    expect(screen.queryByText('Traces')).toBeNull();
    // Six silent days is "stale", not an incident.
    expect(screen.getByText('Stale')).not.toBeNull();
    expect(screen.getByText('Healthy')).not.toBeNull();
  });
});

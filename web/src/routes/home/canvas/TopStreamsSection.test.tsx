import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { HomeOverview, HomeStreamOverview } from '@/api/home';
import type { StreamRuntime, StreamRuntimeOverview } from '@/api/streams';
import i18n from '@/i18n';

import { TopStreamsSection } from './TopStreamsSection';

const NOW = 1_800_000_000_000_000;
const DAY = 86_400_000_000;

function homeStream(
  id: string,
  type: HomeStreamOverview['stream_type'],
): HomeStreamOverview {
  return {
    id,
    name: id,
    stream_type: type,
    status: 'degraded',
    rows: 0,
    stored_bytes: 0,
    first_received_at_micros: null,
    last_received_at_micros: null,
  };
}

function runtime(
  id: string,
  type: StreamRuntime['stream_type'],
  status: StreamRuntime['status'],
  agoMicros: number,
): StreamRuntime {
  return {
    id,
    name: id,
    stream_type: type,
    status,
    rows: 0,
    stored_bytes: 0,
    current_stored_bytes: 0,
    first_received_at_micros: null,
    last_received_at_micros: NOW - agoMicros,
    stats_available: true,
    buckets: [],
  };
}

function overviewOf(streams: HomeStreamOverview[]): HomeOverview {
  return {
    generated_at_micros: NOW,
    window: { start_micros: NOW - DAY, end_micros: NOW, window_secs: 86_400 },
    intake_status: 'degraded',
    probe_reason: null,
    intake_bytes: 0,
    stored_bytes: 0,
    rows: 0,
    compression_savings_ratio: null,
    active_streams: 0,
    total_streams: streams.length,
    attention_streams: streams.length,
    last_received_at_micros: null,
    stats_probe: { succeeded: 1, total: 1 },
    buckets: [],
    signals: [],
    streams,
  };
}

function runtimeOf(streams: StreamRuntime[]): StreamRuntimeOverview {
  return {
    generated_at_micros: NOW,
    window_start_micros: NOW - DAY,
    window_end_micros: NOW,
    window_secs: 86_400,
    streams,
  };
}

function renderTable(
  streams: Array<[HomeStreamOverview, StreamRuntime]>,
  props: Partial<React.ComponentProps<typeof TopStreamsSection>> = {},
) {
  const callbacks = {
    onOpen: vi.fn(),
    onViewAll: vi.fn(),
    onConnect: vi.fn(),
  };
  const view = render(
    <MemoryRouter>
      <TopStreamsSection
        overview={overviewOf(streams.map(([home]) => home))}
        runtimeOverview={runtimeOf(streams.map(([, rt]) => rt))}
        state={null}
        error={null}
        {...callbacks}
        {...props}
      />
    </MemoryRouter>,
  );
  return { ...callbacks, ...view };
}

const STALE: Array<[HomeStreamOverview, StreamRuntime]> = [
  [homeStream('app_logs', 'logs'), runtime('app_logs', 'logs', 'interrupted', 6 * DAY)],
  [
    homeStream('http_requests', 'metrics'),
    runtime('http_requests', 'metrics', 'interrupted', 6 * DAY),
  ],
];

afterEach(cleanup);

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
});

describe('TopStreamsSection', () => {
  it('makes each stream name a real link to where the stream is explored', () => {
    renderTable(STALE);

    expect(screen.getByRole('link', { name: 'app_logs' }).getAttribute('href')).toBe(
      '/logs?stream=app_logs',
    );
    expect(
      screen.getByRole('link', { name: 'http_requests' }).getAttribute('href'),
    ).toBe('/metrics?metric=http_requests');
  });

  it('labels signal types with the product names, as neutral tags', () => {
    renderTable(STALE);

    expect(screen.getByText('Logs')).not.toBeNull();
    expect(screen.getByText('Metrics')).not.toBeNull();
  });

  it('drops the pills to quiet text when every row has the same condition', () => {
    renderTable(STALE);

    const cells = screen.getAllByText('Stale');
    expect(cells).toHaveLength(2);
    for (const cell of cells) {
      expect(cell.closest('td')?.querySelector('.h-\\[22px\\]')).toBeNull();
    }
  });

  it('keeps the pills when conditions differ, because there is something to tell apart', () => {
    renderTable([
      STALE[0]!,
      [homeStream('sample', 'traces'), runtime('sample', 'traces', 'healthy', 1_000_000)],
    ]);

    const healthy = screen.getByText('Healthy');
    expect(healthy.closest('.h-\\[22px\\]')).not.toBeNull();
  });

  it('opens the stream from the row, but lets the link handle its own click', () => {
    const { onOpen, container } = renderTable(STALE);

    const row = container.querySelector('tbody tr')!;
    fireEvent.click(within(row as HTMLElement).getByText('Stale'));
    expect(onOpen).toHaveBeenCalledOnce();
    expect(onOpen.mock.calls[0]?.[0]).toMatchObject({ id: 'app_logs' });

    onOpen.mockClear();
    fireEvent.click(screen.getByRole('link', { name: 'app_logs' }));
    expect(onOpen).not.toHaveBeenCalled();
  });

  it('never gives the table its own vertical scrollbar', () => {
    const { container } = renderTable(STALE);

    const scroller = container
      .querySelector('[data-testid="home-top-streams-viewport"]')!
      .querySelector(':scope > div')!;
    expect(scroller.className).toContain('overflow-y-hidden');
  });

  it('hides the action column header from sight but keeps it for assistive tech', () => {
    renderTable(STALE);
    expect(screen.getByText('Action').className).toContain('sr-only');
  });

  it('offers to connect a source when no streams exist', () => {
    const { onConnect } = renderTable([], { state: 'empty' });

    fireEvent.click(screen.getByRole('button', { name: 'Connect a data source' }));
    expect(onConnect).toHaveBeenCalledOnce();
  });

  it('survives a backend that answers with empty bodies', () => {
    render(
      <MemoryRouter>
        <TopStreamsSection
          overview={{} as HomeOverview}
          runtimeOverview={{} as StreamRuntimeOverview}
          state="empty"
          error={null}
          onOpen={vi.fn()}
          onViewAll={vi.fn()}
          onConnect={vi.fn()}
        />
      </MemoryRouter>,
    );

    expect(screen.getByText('No streams registered yet.')).not.toBeNull();
  });

  it('shows structure-matching placeholders while loading', () => {
    renderTable([], { state: 'loading' });
    expect(screen.getByRole('status').getAttribute('aria-busy')).toBe('true');
  });
});

import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import i18n from '@/i18n';

import { HealthHero, type HealthHeroActions } from './HealthHero';
import { type FleetSummary, summarizeFleet } from './streamCondition';

const NOW = 1_800_000_000_000_000;
const DAY = 86_400_000_000;

function fleetOf(
  streams: Array<{
    status: Parameters<typeof summarizeFleet>[0][number]['status'];
    agoMicros: number | null;
  }>,
): FleetSummary {
  return summarizeFleet(
    streams.map((stream) => ({
      status: stream.status,
      last_received_at_micros:
        stream.agoMicros == null ? null : NOW - stream.agoMicros,
    })),
    NOW,
  );
}

function renderHero({
  fleet,
  loading = false,
  askAgent,
}: {
  fleet: FleetSummary | undefined;
  loading?: boolean;
  askAgent?: HealthHeroActions['askAgent'];
}) {
  const actions = {
    investigate: vi.fn(),
    viewStreams: vi.fn(),
    connect: vi.fn(),
    retry: vi.fn(),
    askAgent,
  } satisfies HealthHeroActions;
  render(
    <HealthHero
      fleet={fleet}
      loading={loading}
      lastReceived="6 days ago"
      actions={actions}
    />,
  );
  return actions;
}

afterEach(cleanup);

beforeEach(async () => {
  await i18n.changeLanguage('en-us');
});

describe('HealthHero', () => {
  it('turns a fleet that has been silent for days into one stale verdict with one next step', () => {
    const actions = renderHero({
      fleet: fleetOf(
        Array.from({ length: 15 }, () => ({
          status: 'interrupted' as const,
          agoMicros: 6 * DAY,
        })),
      ),
    });

    expect(screen.getByRole('heading', { name: 'Data is stale' })).not.toBeNull();
    expect(
      screen.getByText('All 15 streams have had no data for over 24 hours.'),
    ).not.toBeNull();
    expect(screen.getByText('Last received: 6 days ago')).not.toBeNull();

    fireEvent.click(screen.getByRole('button', { name: /Check data intake/ }));
    expect(actions.investigate).toHaveBeenCalledOnce();
  });

  it('counts the affected streams when only some are in the worst state', () => {
    renderHero({
      fleet: fleetOf([
        { status: 'healthy', agoMicros: 1_000_000 },
        { status: 'interrupted', agoMicros: 60 * 60 * 1_000_000 },
        { status: 'interrupted', agoMicros: 2 * 60 * 60 * 1_000_000 },
      ]),
    });

    expect(screen.getByRole('heading', { name: 'Streams interrupted' })).not.toBeNull();
    expect(
      screen.getByText('2 of 3 streams stopped receiving data.'),
    ).not.toBeNull();
  });

  it('offers a way to view streams, not an alarm, when everything is healthy', () => {
    const actions = renderHero({
      fleet: fleetOf([
        { status: 'healthy', agoMicros: 1_000_000 },
        { status: 'healthy', agoMicros: 2_000_000 },
      ]),
      askAgent: vi.fn(),
    });

    expect(screen.getByRole('heading', { name: 'Intake is healthy' })).not.toBeNull();
    // Nothing to explain, so no agent offer.
    expect(screen.queryByRole('button', { name: /Ask Mole Agent/ })).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: /View streams/ }));
    expect(actions.viewStreams).toHaveBeenCalledOnce();
  });

  it('points a workspace with no streams at connecting a source', () => {
    const actions = renderHero({ fleet: fleetOf([]) });

    expect(screen.getByRole('heading', { name: 'No data connected yet' })).not.toBeNull();
    // No last-receive line when there is nothing to have received.
    expect(screen.queryByText(/Last received/)).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: /Connect a data source/ }));
    expect(actions.connect).toHaveBeenCalledOnce();
  });

  it('says so when status cannot be read instead of guessing', () => {
    const actions = renderHero({ fleet: undefined });

    expect(screen.getByRole('heading', { name: 'Status unknown' })).not.toBeNull();
    fireEvent.click(screen.getByRole('button', { name: /Try again/ }));
    expect(actions.retry).toHaveBeenCalledOnce();
  });

  it('hands the agent the verdict it is looking at, and only when something needs explaining', () => {
    const askAgent = vi.fn();
    renderHero({
      fleet: fleetOf([{ status: 'interrupted', agoMicros: 3 * DAY }]),
      askAgent,
    });

    fireEvent.click(screen.getByRole('button', { name: /Ask Mole Agent/ }));
    expect(askAgent).toHaveBeenCalledOnce();
    const prompt = String(askAgent.mock.calls[0]?.[0]);
    expect(prompt).toContain('Data is stale');
    // The verdict's own sentence rides along, without its full stop doubling up.
    expect(prompt).toContain('(All 1 streams have had no data for over 24 hours).');
  });

  describe('state treatment', () => {
    const noActions = {
      investigate: vi.fn(),
      viewStreams: vi.fn(),
      connect: vi.fn(),
      retry: vi.fn(),
    };
    const cases: Array<{
      name: string;
      fleet: FleetSummary;
      color: string;
      pulses: boolean;
    }> = [
      {
        name: 'healthy',
        fleet: fleetOf([{ status: 'healthy', agoMicros: 1_000_000 }]),
        color: 'text-green',
        pulses: false,
      },
      {
        name: 'delayed',
        fleet: fleetOf([{ status: 'delayed', agoMicros: 10 * 60 * 1_000_000 }]),
        color: 'text-yellow',
        pulses: true,
      },
      {
        name: 'stale',
        fleet: fleetOf([{ status: 'interrupted', agoMicros: 3 * DAY }]),
        color: 'text-yellow',
        pulses: true,
      },
      {
        name: 'interrupted',
        fleet: fleetOf([{ status: 'interrupted', agoMicros: 60 * 60 * 1_000_000 }]),
        color: 'text-red',
        pulses: true,
      },
      {
        name: 'no streams',
        fleet: fleetOf([]),
        color: 'text-tx-0',
        pulses: false,
      },
    ];

    it.each(cases)(
      'colors the headline and pulses the dot for $name as designed',
      ({ fleet, color, pulses }) => {
        const { container } = render(
          <HealthHero
            fleet={fleet}
            loading={false}
            lastReceived="a while ago"
            actions={noActions}
          />,
        );
        expect(screen.getByRole('heading', { level: 2 }).classList.contains(color)).toBe(true);
        expect(
          container.querySelector('.home-status-dot')?.classList.contains('home-breathe'),
        ).toBe(pulses);
      },
    );
  });

  it('keeps the verdict compact: the headline and its actions share a row', () => {
    const { container } = render(
      <HealthHero
        fleet={fleetOf([{ status: 'delayed', agoMicros: 60_000_000 }])}
        loading={false}
        lastReceived="a minute ago"
        actions={{
          investigate: vi.fn(),
          viewStreams: vi.fn(),
          connect: vi.fn(),
          retry: vi.fn(),
          askAgent: vi.fn(),
        }}
      />,
    );

    const heading = screen.getByRole('heading', { level: 2 });
    const action = screen.getByRole('button', { name: /Review delayed streams/ });
    const row = heading.closest('.flex-wrap.justify-between');
    expect(row).not.toBeNull();
    expect(row?.contains(action)).toBe(true);
    // No separate eyebrow line above the headline.
    expect(container.querySelectorAll('h2')).toHaveLength(1);
  });

  it('shows a placeholder and no actions while loading', () => {
    renderHero({ fleet: undefined, loading: true });

    expect(screen.queryByRole('button')).toBeNull();
    expect(
      screen.getByRole('region', { name: 'Data intake' }).getAttribute('aria-busy'),
    ).toBe('true');
  });
});

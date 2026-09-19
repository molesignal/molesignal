import { describe, expect, it } from 'vitest';

import type {
  IncidentImpact,
  PublicStatusPageIncident,
  PublicStatusPageSnapshot,
} from '@/api/statusPages';

import {
  archiveMonthWindow,
  incidentHistoryMonths,
  uptimeHistoryMonths,
} from './publicStatusHistory';

const generatedAt = Date.UTC(2026, 7, 10, 12) * 1_000;

function incident(
  id: string,
  impact: IncidentImpact,
  resolvedAt: number,
): PublicStatusPageIncident {
  return {
    id,
    kind: impact === 'maintenance' ? 'maintenance' : 'incident',
    title: id,
    impact,
    status: 'resolved',
    component_ids: ['api'],
    updates: [],
    started_at: resolvedAt - 60 * 60 * 1_000_000,
    ended_at: resolvedAt,
  };
}

function snapshot(history: PublicStatusPageIncident[]): PublicStatusPageSnapshot {
  return {
    page: {
      name: 'Acme',
      slug: 'acme',
      logo_url: null,
      brand_color: '#4F46E5',
      timezone: 'UTC',
      language: 'en-us',
      languages: ['en-us'],
      history_days: 90,
      visibility: 'public',
    },
    overall_status: 'operational',
    components: [],
    component_status_events: [],
    active_incidents: [],
    scheduled_maintenance: [],
    history,
    updated_at: generatedAt,
    generated_at: generatedAt,
  };
}

describe('public status history', () => {
  it('shows three natural months and moves the window one month at a time', () => {
    const current = archiveMonthWindow(generatedAt, 90, 'UTC', null);
    expect(current.keys).toEqual(['2026-06', '2026-07', '2026-08']);
    expect(current.canPrevious).toBe(true);
    expect(current.canNext).toBe(false);

    const previous = archiveMonthWindow(
      generatedAt,
      90,
      'UTC',
      current.endIndex - 1,
    );
    expect(previous.keys).toEqual(['2026-05', '2026-06', '2026-07']);
    expect(previous.canPrevious).toBe(false);
    expect(previous.canNext).toBe(true);
  });

  it('groups resolved incidents by natural month and ranks severity first', () => {
    const august = Date.UTC(2026, 7, 5, 12) * 1_000;
    const months = incidentHistoryMonths(
      snapshot([
        incident('minor', 'minor', august + 3),
        incident('critical', 'critical', august),
        incident('maintenance', 'maintenance', august + 4),
        incident('major', 'major', august + 2),
        incident('july', 'minor', Date.UTC(2026, 6, 31, 12) * 1_000),
      ]),
      'UTC',
      'en-us',
    );

    expect(months.map((month) => month.key)).toEqual(['2026-08', '2026-07']);
    expect(months[0]?.incidents.slice(0, 3).map(({ id }) => id)).toEqual([
      'critical',
      'major',
      'minor',
    ]);
  });

  it('builds full natural-month uptime calendars from daily status', () => {
    const months = uptimeHistoryMonths(
      {
        percentage: 99,
        days: [
          {
            startAt: Date.UTC(2026, 6, 31) * 1_000,
            endAt: Date.UTC(2026, 7, 1) * 1_000,
            status: 'operational',
            hasData: true,
            downtimeMicros: 0,
            eligibleMicros: 24 * 60 * 60 * 1_000_000,
            unavailableMicros: 0,
            incidents: [],
          },
          {
            startAt: Date.UTC(2026, 7, 1) * 1_000,
            endAt: Date.UTC(2026, 7, 2) * 1_000,
            status: 'partial_outage',
            hasData: true,
            downtimeMicros: 60 * 60 * 1_000_000,
            eligibleMicros: 24 * 60 * 60 * 1_000_000,
            unavailableMicros: 60 * 60 * 1_000_000,
            incidents: [{ id: 'incident', title: 'API errors' }],
          },
        ],
      },
      'UTC',
      'en-us',
    );

    expect(months.map((month) => month.key)).toEqual(['2026-07', '2026-08']);
    expect(months[1]?.days).toHaveLength(31);
    expect(months[1]?.days[0]?.status).toBe('partial_outage');
    expect(months[1]?.percentage).toBeLessThan(100);
  });
});

import { describe, expect, it } from 'vitest';

import type {
  PublicStatusPageComponent,
  PublicStatusPageIncident,
  PublicStatusPageSnapshot,
} from '@/api/statusPages';

import { componentCalendarUptime, componentUptime } from './componentUptime';

const DAY = 24 * 60 * 60 * 1_000_000;
const NOW = Date.UTC(2026, 7, 10, 8) * 1_000;

const component: PublicStatusPageComponent = {
  id: 'api',
  name: 'API',
  description: '',
  status: 'operational',
};

function incident(
  id: string,
  startedAt: number,
  resolvedAt: number | null,
  impact: PublicStatusPageIncident['impact'] = 'major',
): PublicStatusPageIncident {
  return {
    id,
    kind: impact === 'maintenance' ? 'maintenance' : 'incident',
    title: id,
    impact,
    status: resolvedAt ? 'resolved' : 'in_progress',
    component_ids: ['api'],
    updates: [],
    started_at: startedAt,
    ended_at: resolvedAt,
  };
}

function snapshot(
  overrides: Partial<PublicStatusPageSnapshot> = {},
): PublicStatusPageSnapshot {
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
    components: [component],
    component_status_events: [
      {
        component_id: 'api',
        status: 'operational',
        started_at: NOW - 90 * DAY,
        ended_at: null,
      },
    ],
    active_incidents: [],
    scheduled_maintenance: [],
    history: [],
    updated_at: NOW - DAY,
    generated_at: NOW,
    ...overrides,
  };
}

describe('componentUptime', () => {
  it('returns a fully operational 90-day window without incidents', () => {
    const result = componentUptime(component, snapshot());

    expect(result.days).toHaveLength(90);
    expect(result.days.every((day) => day.status === 'operational')).toBe(true);
    expect(result.days.every((day) => day.downtimeMicros === 0)).toBe(true);
    expect(result.percentage).toBe(100);
  });

  it('derives daily severity and uptime from overlapping public incidents', () => {
    const result = componentUptime(
      component,
      snapshot({
        history: [
          incident('major', NOW - 30 * 60 * 60 * 1_000_000, NOW - 10 * 60 * 60 * 1_000_000),
          incident('critical', NOW - 20 * 60 * 60 * 1_000_000, NOW - 18 * 60 * 60 * 1_000_000, 'critical'),
        ],
      }),
    );

    expect(result.days.at(-2)?.status).toBe('partial_outage');
    expect(result.days.at(-1)?.status).toBe('major_outage');
    expect(result.days.at(-1)?.downtimeMicros).toBe(14 * 60 * 60 * 1_000_000);
    expect(result.days.at(-1)?.incidents.map(({ id }) => id)).toEqual([
      'major',
      'critical',
    ]);
    expect(result.percentage ?? 0).toBeCloseTo(100 - (20 / (90 * 24)) * 100, 5);
  });

  it('ignores future maintenance and reflects a manual status change without an incident', () => {
    const maintenance = incident('future', NOW + DAY, null, 'maintenance');
    const result = componentUptime(
      { ...component, status: 'degraded_performance' },
      snapshot({
        component_status_events: [
          {
            component_id: 'api',
            status: 'operational',
            started_at: NOW - 90 * DAY,
            ended_at: NOW - 4 * 60 * 60 * 1_000_000,
          },
          {
            component_id: 'api',
            status: 'degraded_performance',
            started_at: NOW - 4 * 60 * 60 * 1_000_000,
            ended_at: null,
          },
        ],
        scheduled_maintenance: [maintenance],
      }),
    );

    expect(result.days.slice(0, -1).every((day) => day.status === 'operational')).toBe(true);
    expect(result.days.at(-1)?.status).toBe('degraded_performance');
    expect(result.days.at(-1)?.incidents).toEqual([]);
    expect(result.days.at(-1)?.downtimeMicros).toBe(4 * 60 * 60 * 1_000_000);
    expect(result.percentage ?? 0).toBeCloseTo(100 - (4 / (90 * 24)) * 100, 5);
  });

  it('shows maintenance in history but excludes it from the uptime calculation', () => {
    const result = componentUptime(
      component,
      snapshot({
        component_status_events: [
          {
            component_id: 'api',
            status: 'operational',
            started_at: NOW - 90 * DAY,
            ended_at: NOW - 4 * 60 * 60 * 1_000_000,
          },
          {
            component_id: 'api',
            status: 'maintenance',
            started_at: NOW - 4 * 60 * 60 * 1_000_000,
            ended_at: null,
          },
        ],
      }),
    );

    expect(result.days.at(-1)?.status).toBe('maintenance');
    expect(result.days.at(-1)?.downtimeMicros).toBe(4 * 60 * 60 * 1_000_000);
    expect(result.percentage).toBe(100);
  });

  it('marks days before the first recorded component status as no data', () => {
    const result = componentUptime(
      component,
      snapshot({
        component_status_events: [
          {
            component_id: 'api',
            status: 'operational',
            started_at: NOW - DAY,
            ended_at: null,
          },
        ],
      }),
    );

    expect(result.days[0]?.hasData).toBe(false);
    expect(result.days.at(-1)?.hasData).toBe(true);
    expect(result.percentage).toBe(100);
  });

  it('aligns archive cells to calendar days in the page timezone', () => {
    const result = componentCalendarUptime(
      component,
      snapshot(),
      2,
      'Asia/Shanghai',
    );

    expect(result.days).toHaveLength(2);
    expect(result.days[0]?.startAt).toBe(Date.UTC(2026, 7, 8, 16) * 1_000);
    expect(result.days[1]?.startAt).toBe(Date.UTC(2026, 7, 9, 16) * 1_000);
    expect(result.days[1]?.endAt).toBe(NOW);
  });
});

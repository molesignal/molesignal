import { describe, expect, it } from 'vitest';

import type { Schedule } from '@/types/alerting';

import {
  MICROS_PER_DAY,
  buildScheduleTimeline,
  nextScheduleBoundary,
  resolveScheduleAt,
  scheduleStatus,
} from './model';

const start = Date.UTC(2026, 6, 25, 10) * 1000;

function fixture(overrides: Partial<Schedule> = {}): Schedule {
  return {
    id: 'schedule-1',
    org_id: 'org-1',
    name: 'production primary on-call',
    description: '',
    team_id: null,
    timezone: 'UTC',
    enabled: true,
    rotations: [
      {
        id: 'rotation-1',
        name: 'primary',
        members: ['u1', 'u2'],
        kind: 'daily',
        active_window: null,
        start_at: start,
      },
    ],
    overrides: [],
    created_by: 'u1',
    updated_by: 'u1',
    created_at: start,
    updated_at: start,
    ...overrides,
  };
}

describe('scheduleModel', () => {
  it('does not resolve a paused schedule', () => {
    expect(resolveScheduleAt(fixture({ enabled: false }), start)).toBeNull();
    expect(scheduleStatus(fixture({ enabled: false }), start)).toBe('paused');
  });

  it('rotates members daily and exposes the next handoff', () => {
    expect(resolveScheduleAt(fixture(), start)?.userId).toBe('u1');
    expect(
      resolveScheduleAt(fixture(), start + MICROS_PER_DAY)?.userId,
    ).toBe('u2');
    expect(nextScheduleBoundary(fixture(), start)).toBe(
      start + MICROS_PER_DAY,
    );
  });

  it('gives an active temporary override priority', () => {
    const schedule = fixture({
      overrides: [
        {
          id: 'override-1',
          user_id: 'u3',
          start_at: start - 1,
          end_at: start + 1_000_000,
          reason: 'cover',
        },
      ],
    });
    expect(resolveScheduleAt(schedule, start)).toMatchObject({
      userId: 'u3',
      source: 'override',
    });
  });

  it('marks an empty active schedule as a coverage gap', () => {
    expect(
      scheduleStatus(fixture({ rotations: [] }), start),
    ).toBe('gap');
  });

  it('builds handoff segments for the requested horizon', () => {
    const timeline = buildScheduleTimeline(fixture(), start, 3);
    expect(timeline).toHaveLength(3);
    expect(timeline.map((segment) => segment.userId)).toEqual([
      'u1',
      'u2',
      'u1',
    ]);
    expect(timeline.at(-1)?.endAt).toBe(start + 3 * MICROS_PER_DAY);
  });
});

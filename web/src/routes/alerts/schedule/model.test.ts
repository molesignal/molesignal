import { describe, expect, it } from 'vitest';

import type { CoverageSegment, Schedule } from '@/types/alerting';

import {
  MICROS_PER_DAY,
  buildScheduleTimeline,
  nextScheduleBoundary,
  resolutionStartedAt,
  resolveScheduleAt,
  rotationKindKey,
  scheduleStatus,
} from './model';

const start = Date.UTC(2026, 6, 25, 10) * 1000;

function rotation(user: string, from: number, to: number): CoverageSegment {
  return {
    startAt: from,
    endAt: to,
    source: 'rotation',
    userId: user,
    rotationId: 'rotation-1',
  };
}

function fixture(overrides: Partial<Schedule> = {}): Schedule {
  return {
    id: 'schedule-1',
    orgId: 'org-1',
    name: 'production primary on-call',
    description: '',
    teamId: null,
    timezone: 'UTC',
    enabled: true,
    rotations: [
      {
        id: 'rotation-1',
        name: 'primary',
        members: ['u1', 'u2'],
        kind: 'daily',
        activeWindows: [],
        startAt: start,
      },
    ],
    overrides: [],
    createdBy: 'u1',
    updatedBy: 'u1',
    createdAt: start,
    updatedAt: start,
    coverage: {
      from: start - MICROS_PER_DAY,
      to: start + 14 * MICROS_PER_DAY,
      segments: [
        { startAt: start - MICROS_PER_DAY, endAt: start, source: 'gap' },
        rotation('u1', start, start + MICROS_PER_DAY),
        rotation('u2', start + MICROS_PER_DAY, start + 2 * MICROS_PER_DAY),
        rotation('u1', start + 2 * MICROS_PER_DAY, start + 14 * MICROS_PER_DAY),
      ],
    },
    ...overrides,
  };
}

describe('scheduleModel', () => {
  it('does not resolve a paused schedule', () => {
    expect(resolveScheduleAt(fixture({ enabled: false }), start)).toBeNull();
    expect(scheduleStatus(fixture({ enabled: false }), start)).toBe('paused');
  });

  it('reads who is on call and the next handoff from the coverage', () => {
    const schedule = fixture();
    expect(resolveScheduleAt(schedule, start)?.userId).toBe('u1');
    expect(resolveScheduleAt(schedule, start + MICROS_PER_DAY)).toMatchObject({
      userId: 'u2',
      source: 'rotation',
      rotationId: 'rotation-1',
    });
    expect(nextScheduleBoundary(schedule, start)).toBe(start + MICROS_PER_DAY);
    expect(
      resolutionStartedAt(schedule, resolveScheduleAt(schedule, start + 5), start + 5),
    ).toBe(start);
  });

  it('knows nobody outside the covered stretch or in a gap', () => {
    const schedule = fixture();
    expect(resolveScheduleAt(schedule, start - 1)).toBeNull();
    expect(resolveScheduleAt(schedule, start + 20 * MICROS_PER_DAY)).toBeNull();
    expect(scheduleStatus(fixture({ coverage: undefined }), start)).toBe('gap');
  });

  it('reports an override the server put in the coverage', () => {
    const schedule = fixture({
      coverage: {
        from: start,
        to: start + MICROS_PER_DAY,
        segments: [
          {
            startAt: start,
            endAt: start + MICROS_PER_DAY,
            source: 'override',
            userId: 'u3',
            overrideId: 'override-1',
          },
        ],
      },
    });
    expect(resolveScheduleAt(schedule, start)).toMatchObject({
      userId: 'u3',
      source: 'override',
      overrideId: 'override-1',
    });
  });

  it('marks an empty active schedule as a coverage gap', () => {
    expect(scheduleStatus(fixture({ rotations: [] }), start)).toBe('gap');
  });

  it('clips the coverage to the requested horizon', () => {
    const timeline = buildScheduleTimeline(fixture(), start, 3);
    expect(timeline.map((segment) => segment.userId)).toEqual(['u1', 'u2', 'u1']);
    expect(timeline[0]?.startAt).toBe(start);
    expect(timeline.at(-1)?.endAt).toBe(start + 3 * MICROS_PER_DAY);
  });

  it('names a whole-day weekday rotation as workdays', () => {
    const [primary] = fixture().rotations;
    expect(rotationKindKey(primary!)).toBe('daily');
    expect(
      rotationKindKey({
        ...primary!,
        activeWindows: [{ weekdayMask: 62, start: '00:00', end: '24:00' }],
      }),
    ).toBe('workdays');
    expect(
      rotationKindKey({
        ...primary!,
        activeWindows: [{ weekdayMask: 62, start: '09:30', end: '18:00' }],
      }),
    ).toBe('daily');
  });
});

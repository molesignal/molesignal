import { describe, expect, it } from 'vitest';

import { MICROS_PER_DAY } from '@/routes/alerts/schedule/model';
import type { CoverageSegment, Schedule } from '@/types/alerting';

import { myShift } from './myShift';

const start = Date.UTC(2026, 6, 25, 10) * 1000;

function held(user: string, fromDay: number, toDay: number): CoverageSegment {
  return {
    startAt: start + fromDay * MICROS_PER_DAY,
    endAt: start + toDay * MICROS_PER_DAY,
    source: 'rotation',
    userId: user,
    rotationId: 'rotation-1',
  };
}

function schedule(id: string, segments: CoverageSegment[], enabled = true): Schedule {
  return {
    id,
    orgId: 'org-1',
    name: id,
    description: '',
    teamId: null,
    timezone: 'UTC',
    enabled,
    rotations: [],
    overrides: [],
    createdBy: 'u1',
    updatedBy: 'u1',
    createdAt: start,
    updatedAt: start,
    coverage: { from: start, to: start + 30 * MICROS_PER_DAY, segments },
  };
}

describe('myShift', () => {
  it('finds the shift under way, held across segments', () => {
    const shift = myShift(
      [schedule('primary', [held('me', 0, 1), held('me', 1, 2), held('ada', 2, 3)])],
      'me',
      start + MICROS_PER_DAY / 2,
    );

    expect(shift).toMatchObject({ now: true, endAt: start + 2 * MICROS_PER_DAY });
  });

  it('takes the soonest next shift across schedules', () => {
    const shift = myShift(
      [
        schedule('late', [held('ada', 0, 5), held('me', 5, 6)]),
        schedule('soon', [held('ada', 0, 2), held('me', 2, 3)]),
      ],
      'me',
      start,
    );

    expect(shift).toMatchObject({ now: false, startAt: start + 2 * MICROS_PER_DAY });
    expect(shift?.schedule.id).toBe('soon');
  });

  it('has nothing to say of paused schedules or of shifts that are not the reader’s', () => {
    expect(myShift([schedule('paused', [held('me', 0, 1)], false)], 'me', start)).toBeNull();
    expect(myShift([schedule('primary', [held('ada', 0, 9)])], 'me', start)).toBeNull();
  });
});

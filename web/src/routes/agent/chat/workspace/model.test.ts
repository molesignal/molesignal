import { describe, expect, it } from 'vitest';

import type { Schedule } from '@/types/alerting';

import { buildOnCallSnapshot } from './model';

function schedule(id: string, name: string, userId: string): Schedule {
  return {
    id,
    orgId: 'org-1',
    name,
    description: '',
    timezone: 'UTC',
    enabled: true,
    rotations: [
      {
        id: `${id}-rotation`,
        name,
        members: [userId],
        kind: 'daily',
        activeWindows: [],
        startAt: 1_000_000,
      },
    ],
    overrides: [],
    createdAt: 1,
    updatedAt: 1,
    coverage: {
      from: 0,
      to: 100_000_000,
      segments: [
        { startAt: 0, endAt: 1_000_000, source: 'gap' },
        {
          startAt: 1_000_000,
          endAt: 100_000_000,
          source: 'rotation',
          userId: userId,
          rotationId: `${id}-rotation`,
        },
      ],
    },
  };
}

describe('agent workspace model', () => {
  it('resolves the primary on-call schedule', () => {
    const snapshot = buildOnCallSnapshot(
      [
        schedule('primary', 'Production primary', 'alice'),
        schedule('secondary', 'Production backup', 'bob'),
      ],
      2_000_000,
    );

    expect(snapshot).toMatchObject({
      scheduleId: 'primary',
      primaryUserId: 'alice',
    });
  });
});

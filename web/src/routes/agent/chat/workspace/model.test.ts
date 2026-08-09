import { describe, expect, it } from 'vitest';

import type { Schedule } from '@/types/alerting';

import { buildOnCallSnapshot } from './model';

function schedule(id: string, name: string, userId: string): Schedule {
  return {
    id,
    org_id: 'org-1',
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
        start_at: 1_000_000,
      },
    ],
    overrides: [],
    created_at: 1,
    updated_at: 1,
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

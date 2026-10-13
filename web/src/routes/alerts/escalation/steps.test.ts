import { describe, expect, it } from 'vitest';

import type { EscalationStep } from '@/types/alerting';

import {
  emptyStep,
  emptyTarget,
  secsLabel,
  stepTargetNames,
  targetRefId,
  withRefId,
} from './steps';

describe('escalation steps', () => {
  it('writes a wait in the largest whole unit', () => {
    expect(secsLabel(60)).toBe('1m');
    expect(secsLabel(900)).toBe('15m');
    expect(secsLabel(3600)).toBe('1h');
    expect(secsLabel(90)).toBe('90s');
    expect(secsLabel(0)).toBe('0s');
  });

  it('reads and sets what a target points at, whichever kind it is', () => {
    expect(targetRefId({ kind: 'user', user_id: 'u1' })).toBe('u1');
    expect(targetRefId({ kind: 'schedule', schedule_id: 's1' })).toBe('s1');
    expect(targetRefId({ kind: 'team', team_id: 't1' })).toBe('t1');
    expect(withRefId({ kind: 'schedule', schedule_id: 's1' }, 's2')).toEqual({ kind: 'schedule', schedule_id: 's2' });
    expect(emptyTarget('team')).toEqual({ kind: 'team', team_id: '' });
    expect(emptyStep()).toEqual({
      targets: [{ kind: 'user', user_id: '' }],
      ack_timeout_secs: 300,
      min_severity: null,
    });
  });

  it('names who a step tells, each by the kind of thing it is', () => {
    const step: EscalationStep = {
      targets: [
        { kind: 'schedule', schedule_id: 'sch-platform' },
        { kind: 'user', user_id: 'u1' },
        { kind: 'team', team_id: 'team-unknown' },
        // A target nobody has picked yet tells nobody.
        { kind: 'user', user_id: '' },
      ],
      ack_timeout_secs: 300,
    };

    expect(
      stepTargetNames(step, {
        user: (id) => ({ u1: 'Dana Reyes' })[id] ?? id,
        schedule: (id) => ({ 'sch-platform': 'Platform on-call' })[id] ?? id,
        team: (id) => id,
      }),
    ).toEqual(['Platform on-call', 'Dana Reyes', 'team-unknown']);
  });
});

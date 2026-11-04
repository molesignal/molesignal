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
    expect(targetRefId({ kind: 'user', userId: 'u1' })).toBe('u1');
    expect(targetRefId({ kind: 'schedule', scheduleId: 's1' })).toBe('s1');
    expect(targetRefId({ kind: 'team', teamId: 't1' })).toBe('t1');
    expect(withRefId({ kind: 'schedule', scheduleId: 's1' }, 's2')).toEqual({ kind: 'schedule', scheduleId: 's2' });
    expect(emptyTarget('team')).toEqual({ kind: 'team', teamId: '' });
    expect(emptyStep()).toEqual({
      targets: [{ kind: 'user', userId: '' }],
      ackTimeoutSecs: 300,
      minSeverity: null,
    });
  });

  it('names who a step tells, each by the kind of thing it is', () => {
    const step: EscalationStep = {
      targets: [
        { kind: 'schedule', scheduleId: 'sch-platform' },
        { kind: 'user', userId: 'u1' },
        { kind: 'team', teamId: 'team-unknown' },
        // A target nobody has picked yet tells nobody.
        { kind: 'user', userId: '' },
      ],
      ackTimeoutSecs: 300,
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

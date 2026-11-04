import { describe, expect, it } from 'vitest';

import type {
  CoverageSegment,
  EscalationPolicy,
  Incident,
  Schedule,
  ScheduleCoverage,
} from '@/types/alerting';

import { MICROS_PER_DAY } from '../../alerts/schedule/model';
import {
  selectFeaturedOnCall,
  summarizeOnCallShift,
} from './model';

const now = Date.UTC(2026, 6, 25, 10) * 1000;

/**
 * The coverage the server would send: daily handoffs from `now`, an override standing in where
 * it is set (split where whom it replaces changes, as the server does).
 */
function coverage(
  id: string,
  members: string[],
  overrides: Schedule['overrides'],
): ScheduleCoverage {
  const from = now - MICROS_PER_DAY;
  const to = now + 3 * MICROS_PER_DAY;
  const points = [
    ...new Set([
      from,
      now,
      now + MICROS_PER_DAY,
      now + 2 * MICROS_PER_DAY,
      ...overrides.flatMap((override) => [override.startAt, override.endAt]),
    ]),
  ]
    .filter((point) => point >= from && point < to)
    .sort((left, right) => left - right);
  const segments = points.map<CoverageSegment>((start, index) => {
    const end = points[index + 1] ?? to;
    const day = Math.floor((start - now) / MICROS_PER_DAY);
    const rotationUser =
      members.length > 0 && start >= now ? members[day % members.length] : undefined;
    const cover = overrides.find(
      (override) => override.startAt <= start && start < override.endAt,
    );
    if (cover) {
      return {
        startAt: start,
        endAt: end,
        source: 'override',
        userId: cover.userId,
        overrideId: cover.id,
        ...(rotationUser ? { replacedUserId: rotationUser } : {}),
      };
    }
    return rotationUser
      ? { startAt: start, endAt: end, source: 'rotation', userId: rotationUser, rotationId: `${id}-rotation` }
      : { startAt: start, endAt: end, source: 'gap' };
  });
  return { from, to, segments };
}

function fixture(
  id: string,
  members: string[],
  overrides: Schedule['overrides'] = [],
): Schedule {
  return {
    id,
    orgId: 'org-1',
    name: id,
    description: '',
    teamId: null,
    timezone: 'UTC',
    enabled: true,
    rotations: [
      {
        id: `${id}-rotation`,
        name: 'primary',
        members,
        kind: 'daily',
        activeWindows: [],
        startAt: now,
      },
    ],
    overrides,
    createdAt: now,
    updatedAt: now,
    coverage: coverage(id, members, overrides),
  };
}

function incident(
  id: string,
  input: Partial<Incident> = {},
): Incident {
  return {
    id,
    orgId: 'org-1',
    ruleId: 'rule-1',
    escalationPolicyId: 'policy-1',
    status: 'open',
    severity: 'warning',
    summary: id,
    fingerprint: id,
    currentStep: 0,
    currentLoop: 0,
    currentStepStartedAt: now,
    assignees: ['me'],
    createdAt: now + 1,
    labels: {},
    annotations: {},
    traceIds: [],
    hostIds: [],
    affectedServices: [],
    triggeringQuery: null,
    ...input,
  };
}

describe('home on-call selection', () => {
  it('puts a coverage gap ahead of healthy schedules', () => {
    const selected = selectFeaturedOnCall(
      [fixture('healthy', ['me']), fixture('gap', [])],
      'me',
      now,
    );

    expect(selected?.schedule.id).toBe('gap');
    expect(selected?.status).toBe('gap');
  });

  it('prefers the current user over another active schedule', () => {
    const selected = selectFeaturedOnCall(
      [fixture('other', ['u2']), fixture('mine', ['me'])],
      'me',
      now,
    );

    expect(selected?.schedule.id).toBe('mine');
    expect(selected?.isMine).toBe(true);
  });

  it('exposes override, replaced member, and next handoff', () => {
    const selected = selectFeaturedOnCall(
      [
        fixture('override', ['u1', 'u2'], [
          {
            id: 'override-1',
            userId: 'u3',
            startAt: now - 1,
            endAt: now + MICROS_PER_DAY / 2,
            reason: 'cover',
          },
        ]),
      ],
      '',
      now,
    );

    expect(selected?.current?.userId).toBe('u3');
    expect(selected?.activeOverride?.id).toBe('override-1');
    expect(selected?.replacedUserId).toBe('u1');
    expect(selected?.currentStartedAt).toBe(now - 1);
    expect(selected?.nextAt).toBe(now + MICROS_PER_DAY / 2);
    expect(selected?.nextUserId).toBe('u1');
  });

  it('summarizes incidents routed through the current schedule', () => {
    const selected = selectFeaturedOnCall(
      [fixture('primary', ['me'])],
      'me',
      now,
    );
    const policy: EscalationPolicy = {
      id: 'policy-1',
      orgId: 'org-1',
      name: 'Default escalation',
      steps: [
        {
          targets: [
            {
              kind: 'schedule',
              scheduleId: 'primary',
            },
          ],
          ackTimeoutSecs: 300,
        },
      ],
      repeat: false,
      maxLoops: 1,
    };

    expect(selected).not.toBeNull();
    if (!selected) return;

    expect(
      summarizeOnCallShift(
        selected,
        [
          incident('open', { currentStep: 1 }),
          incident('acknowledged', {
            status: 'acknowledged',
            acknowledgedAt: now + 2,
          }),
          incident('unrelated', {
            escalationPolicyId: 'policy-2',
          }),
        ],
        [policy],
      ),
    ).toEqual({
      incidentCount: 2,
      pendingCount: 1,
      acknowledgedCount: 1,
      escalatedCount: 1,
      escalationPolicyNames: ['Default escalation'],
    });
  });
});

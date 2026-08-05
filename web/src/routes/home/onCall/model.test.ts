import { describe, expect, it } from 'vitest';

import type {
  EscalationPolicy,
  Incident,
  Schedule,
} from '@/types/alerting';

import { MICROS_PER_DAY } from '../../alerts/schedule/model';
import {
  selectFeaturedOnCall,
  summarizeOnCallShift,
} from './model';

const now = Date.UTC(2026, 6, 25, 10) * 1000;

function fixture(
  id: string,
  members: string[],
  overrides: Schedule['overrides'] = [],
): Schedule {
  return {
    id,
    org_id: 'org-1',
    name: id,
    description: '',
    team_id: null,
    timezone: 'UTC',
    enabled: true,
    rotations: [
      {
        id: `${id}-rotation`,
        name: 'primary',
        members,
        kind: 'daily',
        active_window: null,
        start_at: now,
      },
    ],
    overrides,
    created_at: now,
    updated_at: now,
  };
}

function incident(
  id: string,
  input: Partial<Incident> = {},
): Incident {
  return {
    id,
    org_id: 'org-1',
    rule_id: 'rule-1',
    escalation_policy_id: 'policy-1',
    status: 'open',
    severity: 'warning',
    summary: id,
    fingerprint: id,
    current_step: 0,
    current_loop: 0,
    current_step_started_at: now,
    assignees: ['me'],
    created_at: now + 1,
    labels: {},
    annotations: {},
    trace_ids: [],
    host_ids: [],
    affected_services: [],
    triggering_query: null,
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
            user_id: 'u3',
            start_at: now - 1,
            end_at: now + MICROS_PER_DAY / 2,
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
      org_id: 'org-1',
      name: 'Default escalation',
      steps: [
        {
          targets: [
            {
              kind: 'schedule',
              schedule_id: 'primary',
            },
          ],
          ack_timeout_secs: 300,
        },
      ],
      repeat: false,
      max_loops: 1,
    };

    expect(selected).not.toBeNull();
    if (!selected) return;

    expect(
      summarizeOnCallShift(
        selected,
        [
          incident('open', { current_step: 1 }),
          incident('acknowledged', {
            status: 'acknowledged',
            acknowledged_at: now + 2,
          }),
          incident('unrelated', {
            escalation_policy_id: 'policy-2',
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

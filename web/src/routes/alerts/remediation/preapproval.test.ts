import { describe, expect, it } from 'vitest';

import type { RemediationAction, RemediationPreapproval } from '@/api/remediation';

import {
  approversNeeded,
  preapprovalDraftOf,
  preapprovalInputOf,
  preapprovalStanding,
} from './preapproval';

const NOW = 1_762_849_800_000_000;
const DAY = 86_400_000_000;

function action(fields: Partial<RemediationAction> = {}): RemediationAction {
  return {
    id: 'act-1',
    name: 'Restart checkout',
    description: '',
    target: { kind: 'http', method: 'POST', url: 'https://ci.example.com/restart', body: '' },
    parameters: [],
    services: ['checkout', 'payments'],
    teamIds: [],
    risk: 'l2',
    verifyAfterSecs: 0,
    verifyBy: 'incident_resolved',
    enabled: true,
    secretsSet: [],
    createdAt: NOW,
    updatedAt: NOW,
    ...fields,
  };
}

function preapproval(fields: Partial<RemediationPreapproval> = {}): RemediationPreapproval {
  return {
    services: ['checkout'],
    maxRuns: 2,
    windowSecs: 3_600,
    expiresAt: NOW + 10 * DAY,
    channels: [],
    approvers: [{ userId: 'u-1', at: NOW }],
    setBy: 'u-1',
    setAt: NOW,
    ...fields,
  };
}

describe('pre-approval drafts', () => {
  it('starts from the action’s services, one run an hour for 30 days', () => {
    expect(preapprovalDraftOf(action(), NOW)).toEqual({
      services: 'checkout, payments',
      maxRuns: 1,
      windowSecs: 3_600,
      expiresInDays: 30,
      channels: [],
    });
  });

  it('opens what is set, the expiry rounded up to a choice', () => {
    const draft = preapprovalDraftOf(action({ preapproval: preapproval() }), NOW);

    expect(draft.services).toBe('checkout');
    expect(draft.maxRuns).toBe(2);
    expect(draft.expiresInDays).toBe(30);
  });

  it('sends trimmed services, a bounded count, an expiry and only channels with a target', () => {
    const input = preapprovalInputOf(
      {
        services: ' checkout, ,payments ',
        maxRuns: 99,
        windowSecs: 900,
        expiresInDays: 7,
        channels: [
          { connectorId: 'feishu-1', targetType: 'fixed_group', target: ' oc_ops ' },
          { connectorId: 'feishu-1', targetType: 'fixed_group', target: '  ' },
        ],
      },
      NOW,
    );

    expect(input).toEqual({
      services: ['checkout', 'payments'],
      maxRuns: 20,
      windowSecs: 900,
      expiresAt: NOW + 7 * DAY,
      channels: [{ connectorId: 'feishu-1', targetType: 'fixed_group', target: 'oc_ops' }],
    });
    expect(preapprovalInputOf({ ...preapprovalDraftOf(action(), NOW), expiresInDays: null }, NOW).expiresAt).toBeNull();
  });
});

describe('where a pre-approval stands', () => {
  it('an L3 action waits for a second person, who is not the first', () => {
    const l3 = action({ risk: 'l3', preapproval: preapproval() });
    const overview = { enabled: true, actions: { 'act-1': { used: 0, state: 'awaiting_approver' as const } } };

    expect(approversNeeded('l3')).toBe(2);
    expect(preapprovalStanding(l3, overview, 'u-2')).toEqual({
      state: 'awaiting_approver',
      used: 0,
      max: 2,
      canConfirm: true,
    });
    expect(preapprovalStanding(l3, overview, 'u-1').canConfirm).toBe(false);
  });

  it('reads the runs used and the state from the overview', () => {
    const l2 = action({ preapproval: preapproval() });
    const overview = { enabled: true, actions: { 'act-1': { used: 2, state: 'used_up' as const } } };

    expect(preapprovalStanding(l2, overview, 'u-2')).toEqual({
      state: 'used_up',
      used: 2,
      max: 2,
      canConfirm: false,
    });
    expect(preapprovalStanding(action(), overview, 'u-2').state).toBe('none');
    expect(preapprovalStanding(l2, { enabled: false, actions: {} }, 'u-2').state).toBe('off');
  });
});

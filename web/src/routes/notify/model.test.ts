import { describe, expect, it } from 'vitest';

import type {
  NotifyConnector,
  UserNotifyEndpoint,
  UserNotifyPreference,
} from '@/api/notify';

import {
  connectorName,
  deliveryStages,
  endpointLabel,
  primaryEndpoint,
  statusTone,
  targetTypeOptions,
} from './model';

const connector: NotifyConnector = {
  id: 'connector-email',
  organizationId: 'org',
  name: 'Company email',
  connectorType: 'email_smtp',
  config: {},
  capabilities: {
    directUser: true,
    group: true,
    richText: true,
    interactive: false,
    acknowledgement: false,
    attachments: false,
  },
  enabled: true,
  status: 'connected',
  createdAt: 1,
  updatedAt: 1,
};

function endpoint(id: string, identity: string): UserNotifyEndpoint {
  return {
    id,
    organizationId: 'org',
    userId: 'user',
    connectorId: connector.id,
    providerType: connector.connectorType,
    externalIdentity: identity,
    verified: true,
    enabled: true,
    metadata: {},
    createdAt: 1,
    updatedAt: 1,
  };
}

describe('notify model', () => {
  it('resolves connector and ordered primary endpoint labels', () => {
    const first = endpoint('endpoint-first', 'owner@example.com');
    const second = endpoint('endpoint-second', 'backup@example.com');
    const preference: UserNotifyPreference = {
      id: 'preference',
      organizationId: 'org',
      userId: 'user',
      category: 'alert',
      enabled: true,
      allowHighUrgencyBypass: true,
      steps: [
        {
          id: 'step-second',
          preferenceId: 'preference',
          endpointId: second.id,
          stepOrder: 2,
          delaySecs: 0,
          createdAt: 1,
        },
        {
          id: 'step-first',
          preferenceId: 'preference',
          endpointId: first.id,
          stepOrder: 1,
          delaySecs: 0,
          createdAt: 1,
        },
      ],
      createdAt: 1,
      updatedAt: 1,
    };

    expect(connectorName([connector], connector.id)).toBe('Company email');
    expect(primaryEndpoint(preference, [second, first])).toBe(first);
    expect(endpointLabel(first, [connector])).toBe(
      'Company email · owner@example.com',
    );
  });

  it('keeps delivery stages and target types aligned with the API', () => {
    expect(deliveryStages()).toEqual([
      'user_primary',
      'user_fallback',
      'team_fallback',
      'organization_fallback',
      'escalation',
      'test',
      // Sent straight to a channel a feature chose itself, such as a scheduled log inspection.
      'direct',
    ]);
    expect(targetTypeOptions()).toEqual([
      'direct_user',
      'fixed_address',
      'fixed_group',
      'webhook',
    ]);
    expect(statusTone('acknowledged')).toBe('green');
    expect(statusTone('failed')).toBe('red');
    expect(statusTone('pending')).toBe('blue');
  });
});

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
  organization_id: 'org',
  name: 'Company email',
  connector_type: 'email_smtp',
  config: {},
  capabilities: {
    direct_user: true,
    group: true,
    rich_text: true,
    interactive: false,
    acknowledgement: false,
    attachments: false,
  },
  enabled: true,
  status: 'connected',
  created_at: 1,
  updated_at: 1,
};

function endpoint(id: string, identity: string): UserNotifyEndpoint {
  return {
    id,
    organization_id: 'org',
    user_id: 'user',
    connector_id: connector.id,
    provider_type: connector.connector_type,
    external_identity: identity,
    verified: true,
    enabled: true,
    metadata: {},
    created_at: 1,
    updated_at: 1,
  };
}

describe('notify model', () => {
  it('resolves connector and ordered primary endpoint labels', () => {
    const first = endpoint('endpoint-first', 'owner@example.com');
    const second = endpoint('endpoint-second', 'backup@example.com');
    const preference: UserNotifyPreference = {
      id: 'preference',
      organization_id: 'org',
      user_id: 'user',
      category: 'alert',
      enabled: true,
      allow_critical_bypass: true,
      steps: [
        {
          id: 'step-second',
          preference_id: 'preference',
          endpoint_id: second.id,
          step_order: 2,
          created_at: 1,
        },
        {
          id: 'step-first',
          preference_id: 'preference',
          endpoint_id: first.id,
          step_order: 1,
          created_at: 1,
        },
      ],
      created_at: 1,
      updated_at: 1,
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

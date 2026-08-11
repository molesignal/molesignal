import { describe, expect, it } from 'vitest';

import type { SyntheticSecret } from '@/api/synthetics';

import { emptyDraft, inputFromDraft } from './model';

const SECRET: SyntheticSecret = {
  id: 'secret-api-key',
  organization_id: 'org-a',
  name: 'API_KEY',
  description: 'Test credential',
  current_version: 2,
  created_by: 'user-a',
  created_at: 1,
  updated_at: 2,
};

describe('synthetics editor model', () => {
  it('keeps runtime variables and encrypted secrets distinct', () => {
    const draft = {
      ...emptyDraft([]),
      name: 'Checkout API',
      url: '{{base_url}}',
      headers: [{ name: 'Authorization', value: '{{API_KEY}}' }],
      locationIds: ['location-a'],
    };

    const input = inputFromDraft(draft, [SECRET]);

    expect(input.spec.kind).toBe('http');
    if (input.spec.kind !== 'http') throw new Error('expected HTTP spec');
    const step = input.spec.configuration.steps[0];
    expect(step?.url).toEqual({ source: 'variable', name: 'base_url' });
    expect(step?.headers[0]?.value).toEqual({
      source: 'secret',
      reference: 'API_KEY',
      secret_id: SECRET.id,
    });
  });

  it('enforces the browser cadence and timeout boundary', () => {
    const input = inputFromDraft({
      ...emptyDraft([]),
      name: 'Checkout journey',
      kind: 'browser',
      browserSteps: 'open https://example.com',
      intervalSeconds: '30',
      timeoutMillis: '90000',
      locationIds: ['location-a'],
    });

    expect(input.schedule).toEqual({ kind: 'interval', every_seconds: 60 });
    expect(input.timeout_millis).toBe(59_999);
  });

  it('creates a location-free heartbeat schedule', () => {
    const input = inputFromDraft({
      ...emptyDraft([]),
      name: 'Batch heartbeat',
      kind: 'heartbeat',
      intervalSeconds: '300',
      locationIds: ['ignored-location'],
    });

    expect(input.spec).toEqual({ kind: 'heartbeat' });
    expect(input.schedule).toEqual({
      kind: 'heartbeat',
      expected_seconds: 300,
      grace_seconds: 600,
    });
    expect(input.location_ids).toEqual([]);
  });
});

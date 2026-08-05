import { describe, expect, it } from 'vitest';

import { apmQueryKeys, type ApmQueryParams } from './apm';

describe('APM query keys', () => {
  it('is stable across object insertion order', () => {
    const first: ApmQueryParams = {
      from: 10,
      to: 20,
      service: 'checkout',
      environment: 'prod',
      version: '2.0.0',
      sort: 'p95',
      cursor: 'next',
    };
    const second: ApmQueryParams = {
      cursor: 'next',
      sort: 'p95',
      version: '2.0.0',
      environment: 'prod',
      service: 'checkout',
      to: 20,
      from: 10,
    };

    expect(apmQueryKeys.transactions('org-a', first)).toEqual(
      apmQueryKeys.transactions('org-a', second),
    );
  });

  it.each([
    ['organization', 'org-b', {}],
    ['time', 'org-a', { from: 11 }],
    ['namespace', 'org-a', { namespace: 'payments' }],
    ['service', 'org-a', { service: 'inventory' }],
    ['environment', 'org-a', { environment: 'staging' }],
    ['version', 'org-a', { version: '3.0.0' }],
    ['transaction kind', 'org-a', { kind: 'rpc' }],
    ['sort', 'org-a', { sort: 'error_rate' }],
    ['cursor', 'org-a', { cursor: 'page-2' }],
  ] as const)('changes when %s changes', (_label, orgId, patch) => {
    const base: ApmQueryParams = {
      from: 10,
      to: 20,
      namespace: 'shop',
      service: 'checkout',
      environment: 'prod',
      version: '2.0.0',
      sort: 'p95',
      cursor: 'page-1',
    };
    expect(apmQueryKeys.services('org-a', base)).not.toEqual(
      apmQueryKeys.services(orgId, { ...base, ...patch }),
    );
  });

  it('includes both comparison versions independently', () => {
    const base = {
      from: 10,
      to: 20,
      service: 'checkout',
      baseline: '1.0.0',
      candidate: '2.0.0',
    };
    expect(apmQueryKeys.compare('org-a', base)).not.toEqual(
      apmQueryKeys.compare('org-a', { ...base, candidate: '2.1.0' }),
    );
  });
});

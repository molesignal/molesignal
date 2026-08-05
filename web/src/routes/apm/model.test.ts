import { describe, expect, it } from 'vitest';

import {
  apiParamsFromFilters,
  parseApmFilters,
  servicePath,
  signalHref,
  transactionPath,
  writeApmFilter,
} from './model';

describe('APM URL model', () => {
  it('round-trips entity, sort, resolution, and cursor filters', () => {
    const search =
      'namespace=shop&service=checkout&environment=prod&version=2.0.0&q=orders&category=database&resolution=hour&sort=p95&direction=asc&cursor=page-2';
    const filters = parseApmFilters(search);
    expect(filters).toEqual({
      namespace: 'shop',
      service: 'checkout',
      environment: 'prod',
      version: '2.0.0',
      search: 'orders',
      category: 'database',
      resolution: 'hour',
      sort: 'p95',
      direction: 'asc',
      cursor: 'page-2',
    });
    expect(
      apiParamsFromFilters(filters, {
        from: new Date('2026-07-30T00:00:00Z'),
        to: new Date('2026-07-30T01:00:00Z'),
      }),
    ).toMatchObject({
      namespace: 'shop',
      service: 'checkout',
      environment: 'prod',
      version: '2.0.0',
      resolution: 'hour',
      sort: 'p95',
      direction: 'asc',
      cursor: 'page-2',
    });
  });

  it('drops pagination whenever scope changes', () => {
    const next = writeApmFilter(
      new URLSearchParams('service=checkout&cursor=page-2'),
      'environment',
      'staging',
    );
    expect(next.get('environment')).toBe('staging');
    expect(next.has('cursor')).toBe(false);
  });

  it('builds scoped service and cross-signal links without raw request data', () => {
    expect(
      servicePath({
        namespace: 'shop',
        name: 'checkout/api',
        environment: 'prod',
      }),
    ).toBe(
      '/apm/services/checkout%2Fapi?namespace=shop&environment=prod',
    );
    const href = signalHref(
      'traces',
      {
        namespace: 'shop',
        service: 'checkout',
        environment: 'prod',
        version: '2.0.0',
        transaction: 'GET /orders/{id}',
        from: 1_000_000,
        to: 2_000_000,
      },
      { traceSort: 'duration_desc' },
    );
    const url = new URL(href, 'https://molesignal.test');
    expect(url.pathname).toBe('/traces');
    expect(url.searchParams.get('transaction')).toBe('GET /orders/{id}');
    expect(url.searchParams.get('version')).toBe('2.0.0');
    expect(url.searchParams.get('sort')).toBe('duration_desc');
    expect(href).not.toContain('token=');
  });

  it('builds a canonical Transaction detail path with an unambiguous identity', () => {
    const path = transactionPath({
      service: {
        namespace: 'shop',
        name: 'checkout',
        environment: 'prod',
      },
      version: '2.0.0',
      transaction: { name: 'POST /orders', kind: 'http' },
      red: {
        request_count: 1,
        error_count: 0,
        error_rate: 0,
        duration_sum_micros: 100,
        latency_partial: false,
        exemplars: [],
      },
      total_time_micros: 100,
      traces: {
        namespace: 'shop',
        service: 'checkout',
        environment: 'prod',
        version: '2.0.0',
        transaction: 'POST /orders',
        from: 1,
        to: 2,
      },
    });

    expect(path).toBe(
      '/apm/transactions/POST%20%2Forders?namespace=shop&service=checkout&environment=prod&kind=http&version=2.0.0',
    );
  });
});

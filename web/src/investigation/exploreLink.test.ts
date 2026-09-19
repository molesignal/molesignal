import { describe, expect, it } from 'vitest';

import type { PanelQuery } from '@/dashboard-engine/schema';

import { buildPanelExploreLink } from './exploreLink';

const timeRange = {
  from: Date.parse('2026-08-14T05:00:00.000Z') * 1_000,
  to: Date.parse('2026-08-14T06:00:00.000Z') * 1_000,
};

function query(
  dataSourceType: PanelQuery['dataSourceType'],
  config: Record<string, unknown>,
): PanelQuery {
  return {
    refId: 'A',
    enabled: true,
    dataSourceType,
    dataSourceId: 'source-1',
    query: config,
  };
}

describe('buildPanelExploreLink', () => {
  it('uses the PromQL contract and carries time, variables, and datasource', () => {
    const href = buildPanelExploreLink({
      query: query('metrics', {
        language: 'promql',
        expression: 'rate(http_requests_total{service="$service"}[5m])',
      }),
      variables: { service: 'checkout' },
      timeRange,
    });
    const url = new URL(href, 'https://molesignal.local');

    expect(url.pathname).toBe('/metrics');
    expect(url.searchParams.get('promql')).toBe(
      'rate(http_requests_total{service="checkout"}[5m])',
    );
    expect(url.searchParams.get('query')).toBeNull();
    expect(url.searchParams.get('time')).toBe(
      '2026-08-14T05:00:00.000Z..2026-08-14T06:00:00.000Z',
    );
    expect(url.searchParams.get('var-service')).toBe('checkout');
    expect(url.searchParams.get('data_source')).toBe('source-1');
  });

  it('puts a Logs statement into the SQL editor contract', () => {
    const statement = 'SELECT * FROM "app_logs" WHERE service = \'checkout\'';
    const href = buildPanelExploreLink({
      query: query('logs', {
        language: 'sql',
        statement,
        stream: 'app_logs',
        streamType: 'logs',
      }),
      variables: {},
      timeRange,
    });
    const url = new URL(href, 'https://molesignal.local');

    expect(url.pathname).toBe('/logs');
    expect(url.searchParams.get('sql')).toBe(statement);
    expect(url.searchParams.get('q')).toBeNull();
    expect(url.searchParams.get('stream')).toBe('app_logs');
    expect(url.searchParams.get('stream_type')).toBe('logs');
  });

  it('puts a Traces statement and stream into the SQL editor contract', () => {
    const statement = 'SELECT * FROM "checkout_traces"';
    const href = buildPanelExploreLink({
      query: query('traces', {
        language: 'sql',
        statement,
        stream: 'checkout_traces',
        streamType: 'traces',
      }),
      variables: {},
      timeRange,
    });
    const url = new URL(href, 'https://molesignal.local');

    expect(url.pathname).toBe('/traces');
    expect(url.searchParams.get('sql')).toBe(statement);
    expect(url.searchParams.get('stream')).toBe('checkout_traces');
    expect(url.searchParams.get('stream_type')).toBe('traces');
  });
});

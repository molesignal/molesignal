import { describe, expect, it } from 'vitest';

import type { HomeOverview } from '@/api/home';

import {
  filterSources,
  integrationMethodForSource,
  maskToken,
  primaryCategoryFromRoute,
  sourceInPrimaryCategory,
  summarizeSourceSignals,
} from './datasourceModel';
import type { Source } from './sources';

function source(overrides: Partial<Source>): Source {
  return {
    id: 'linux',
    name: 'Linux',
    category: 'recommended',
    glyph: 'LX',
    description: 'Linux host integration',
    signals: ['logs'],
    steps: [],
    ...overrides,
  };
}

describe('datasourceModel', () => {
  it('maps old route categories into the simplified catalogue', () => {
    expect(primaryCategoryFromRoute('otel')).toBe('applications');
    expect(primaryCategoryFromRoute('networking')).toBe('infrastructure');
    expect(primaryCategoryFromRoute('cloud')).toBe('cloud');
    expect(primaryCategoryFromRoute('does-not-exist')).toBe('recommended');
  });

  it('groups recommended cloud sources under the cloud catalogue too', () => {
    const aws = source({ id: 'aws', name: 'AWS' });
    const nginx = source({ id: 'nginx', category: 'servers' });
    expect(sourceInPrimaryCategory(aws, 'cloud')).toBe(true);
    expect(sourceInPrimaryCategory(aws, 'recommended')).toBe(true);
    expect(sourceInPrimaryCategory(nginx, 'infrastructure')).toBe(true);
  });

  it('filters by category, method, signal and global search', () => {
    const sources = [
      source({ id: 'linux', name: 'Linux' }),
      source({ id: 'opentelemetry', name: 'OpenTelemetry', category: 'otel', signals: ['traces'] }),
      source({ id: 'webhook', name: 'HTTP Webhook', category: 'custom' }),
    ];
    expect(
      filterSources({
        sources,
        category: 'applications',
        method: 'otel',
        signal: 'traces',
        query: '',
      }).map((item) => item.id),
    ).toEqual(['opentelemetry']);
    expect(
      filterSources({
        sources,
        category: 'recommended',
        method: 'all',
        signal: 'all',
        query: 'webhook',
      }).map((item) => item.id),
    ).toEqual(['webhook']);
    expect(integrationMethodForSource(sources[2]!)).toBe('api');
  });

  it('masks the middle of tokens while keeping them recognizable', () => {
    const token = 'ms_abcd_1234567890_secret';
    const masked = maskToken(token);
    expect(masked).toMatch(/^ms_abcd_/);
    expect(masked).toMatch(/secret$/);
    expect(masked).not.toContain('1234567890');
  });

  it('summarizes only the selected source signal types', () => {
    const overview = {
      signals: [
        {
          stream_type: 'logs',
          status: 'healthy',
          total_streams: 1,
          active_streams: 1,
          rows: 120,
          stored_bytes: 2048,
          last_received_at_micros: 200,
        },
        {
          stream_type: 'metrics',
          status: 'no_data',
          total_streams: 1,
          active_streams: 0,
          rows: 0,
          stored_bytes: 0,
          last_received_at_micros: null,
        },
      ],
      streams: [
        {
          id: 'logs/default',
          name: 'default',
          stream_type: 'logs',
          status: 'healthy',
          rows: 120,
          stored_bytes: 2048,
          first_received_at_micros: 100,
          last_received_at_micros: 200,
        },
      ],
    } as HomeOverview;
    expect(
      summarizeSourceSignals(source({ signals: ['logs', 'metrics'] }), overview),
    ).toEqual({
      status: 'healthy',
      rows: 120,
      storedBytes: 2048,
      lastReceivedAtMicros: 200,
      activeSignals: 1,
      expectedSignals: 2,
      streamNames: ['default'],
    });
  });

  it('handles legacy overview responses without signal or stream arrays', () => {
    const legacyOverview = {} as HomeOverview;

    expect(summarizeSourceSignals(source({ signals: ['traces'] }), legacyOverview)).toEqual({
      status: 'unknown',
      rows: 0,
      storedBytes: 0,
      lastReceivedAtMicros: null,
      activeSignals: 0,
      expectedSignals: 1,
      streamNames: [],
    });
  });
});

import { describe, expect, it } from 'vitest';

import type { HomeOverview } from '@/api/home';

import {
  categoryForSource,
  maskToken,
  primaryCategoryFromRoute,
  searchSources,
  sendsSignal,
  sourceInPrimaryCategory,
  sourcesInCategory,
  summarizeSourceSignals,
} from './datasourceModel';
import type { Source } from './sources';

function source(overrides: Partial<Source>): Source {
  return {
    id: 'linux',
    category: 'recommended',
    signals: ['logs'],
    steps: [],
    ...overrides,
  };
}

const ids = (sources: Source[]) => sources.map((item) => item.id);

describe('datasourceModel', () => {
  it('maps old route categories into the simplified catalogue', () => {
    expect(primaryCategoryFromRoute('otel')).toBe('applications');
    expect(primaryCategoryFromRoute('networking')).toBe('infrastructure');
    expect(primaryCategoryFromRoute('cloud')).toBe('cloud');
    expect(primaryCategoryFromRoute('does-not-exist')).toBe('recommended');
  });

  it('groups recommended cloud sources under the cloud catalogue too', () => {
    const aws = source({ id: 'aws' });
    const nginx = source({ id: 'nginx', category: 'servers' });
    expect(sourceInPrimaryCategory(aws, 'cloud')).toBe(true);
    expect(sourceInPrimaryCategory(aws, 'recommended')).toBe(true);
    expect(sourceInPrimaryCategory(nginx, 'infrastructure')).toBe(true);
  });

  it('lists a category narrowed to the sources that send a signal', () => {
    const sources = [
      source({ id: 'linux', signals: ['logs', 'metrics'] }),
      source({ id: 'kubernetes' }),
      source({ id: 'opentelemetry', category: 'otel', signals: ['logs', 'metrics', 'traces'] }),
    ];

    expect(ids(sourcesInCategory(sources, 'recommended', 'all'))).toEqual(['linux', 'kubernetes']);
    expect(ids(sourcesInCategory(sources, 'recommended', 'metrics'))).toEqual(['linux']);
    expect(ids(sourcesInCategory(sources, 'applications', 'traces'))).toEqual(['opentelemetry']);
    expect(sourcesInCategory(sources, 'recommended', 'profiles')).toEqual([]);
    expect(sendsSignal(sources[0]!, 'all')).toBe(true);
    expect(sendsSignal(sources[1]!, 'metrics')).toBe(false);
  });

  it('keeps a source under the category in view, or sends it to a home of its own', () => {
    const aws = source({ id: 'aws' });
    const postgres = source({ id: 'postgres', category: 'databases' });

    // In Recommended and in Cloud: whichever the reader is in.
    expect(categoryForSource(aws, 'recommended')).toBe('recommended');
    expect(categoryForSource(aws, 'cloud')).toBe('cloud');
    // Reached from a category that does not hold it: its own, never Recommended by default.
    expect(categoryForSource(aws, 'databases')).toBe('cloud');
    expect(categoryForSource(postgres, 'recommended')).toBe('databases');
  });

  it('searches every category by what a source is called in the reader\'s language', () => {
    const sources = [
      source({ id: 'linux' }),
      source({ id: 'postgres', category: 'databases' }),
      source({ id: 'webhook', category: 'custom' }),
    ];
    const words: Record<string, string[]> = {
      linux: ['Linux 主机', '采集 journald'],
      postgres: ['PostgreSQL', '采集慢查询日志'],
      webhook: ['通用 HTTP 接入', '任意服务通过 HTTP POST JSON，可写入 PostgreSQL 之外的数据'],
    };
    const search = (query: string) =>
      ids(searchSources(sources, query, (item) => words[item.id] ?? []));

    expect(search('  ')).toEqual([]);
    expect(search('主机')).toEqual(['linux']);
    expect(search('JOURNALD')).toEqual(['linux']);
    // Matched by name before matched by description, whatever the catalogue order.
    expect(search('postgresql')).toEqual(['postgres', 'webhook']);
    // The identifier is searchable in any language.
    expect(search('webhook')).toEqual(['webhook']);
    expect(search('kafka')).toEqual([]);
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

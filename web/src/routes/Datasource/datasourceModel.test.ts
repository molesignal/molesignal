import { describe, expect, it } from 'vitest';

import type { HomeOverview } from '@/api/home';

import {
  categoryForSource,
  isSignalFilter,
  maskToken,
  primaryCategoryFromRoute,
  searchSources,
  sendsSignal,
  shownSignals,
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
  it('opens the category an address names, and Recommended for any other name', () => {
    expect(primaryCategoryFromRoute('devops')).toBe('devops');
    expect(primaryCategoryFromRoute('cloud')).toBe('cloud');
    expect(primaryCategoryFromRoute('otel')).toBe('recommended');
    expect(primaryCategoryFromRoute(undefined)).toBe('recommended');
  });

  it('groups recommended cloud sources under the cloud catalogue too', () => {
    const aws = source({ id: 'aws' });
    const nginx = source({ id: 'nginx', category: 'servers' });
    expect(sourceInPrimaryCategory(aws, 'cloud')).toBe(true);
    expect(sourceInPrimaryCategory(aws, 'recommended')).toBe(true);
    expect(sourceInPrimaryCategory(nginx, 'infrastructure')).toBe(true);
  });

  it('lists the Alibaba Cloud Prometheus delivery among the clouds, not the infrastructure', () => {
    const aliyun = source({ id: 'aliyun-prometheus', category: 'networking', signals: ['metrics'] });
    expect(sourceInPrimaryCategory(aliyun, 'cloud')).toBe(true);
    expect(sourceInPrimaryCategory(aliyun, 'infrastructure')).toBe(false);
    expect(sourceInPrimaryCategory(aliyun, 'recommended')).toBe(false);

    for (const id of ['tencent-prometheus', 'huawei-prometheus', 'volcengine-cloudmonitor', 'volcengine-prometheus']) {
      expect(sourceInPrimaryCategory(source({ id, category: 'networking', signals: ['metrics'] }), 'cloud')).toBe(true);
    }
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
    expect(sourcesInCategory([source({ id: 'nacos', category: 'devops', signals: ['traces'] })], 'applications', 'all')).toEqual([]);
    expect(ids(sourcesInCategory([source({ id: 'nacos', category: 'devops', signals: ['traces'] })], 'devops', 'all'))).toEqual(['nacos']);
    expect(sourcesInCategory(sources, 'recommended', 'profiles')).toEqual([]);
    expect(sendsSignal(sources[0]!, 'all')).toBe(true);
    expect(sendsSignal(sources[1]!, 'metrics')).toBe(false);
  });

  it('names a RUM source RUM and finds it under RUM, not under the logs it is stored as', () => {
    const rum = source({ id: 'rum', signals: ['logs'], rumPlatform: 'browser' });
    const linux = source({ id: 'linux', signals: ['logs', 'metrics'] });

    expect(shownSignals(rum)).toEqual(['rum']);
    expect(shownSignals(linux)).toEqual(['logs', 'metrics']);
    expect(sendsSignal(rum, 'rum')).toBe(true);
    expect(sendsSignal(rum, 'logs')).toBe(false);
    expect(sendsSignal(linux, 'rum')).toBe(false);
    expect(isSignalFilter('rum')).toBe(true);
    expect(isSignalFilter('events')).toBe(false);
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
    const token = 'mspat-abcd1234567890secret';
    const masked = maskToken(token);
    expect(masked).toMatch(/^mspat-ab/);
    expect(masked).toMatch(/secret$/);
    expect(masked).not.toContain('1234567890');
  });

  it('summarizes only the selected source signal types', () => {
    const overview = {
      signals: [
        {
          streamType: 'logs',
          status: 'healthy',
          totalStreams: 1,
          activeStreams: 1,
          rows: 120,
          storedBytes: 2048,
          lastReceivedAt: 200,
        },
        {
          streamType: 'metrics',
          status: 'no_data',
          totalStreams: 1,
          activeStreams: 0,
          rows: 0,
          storedBytes: 0,
          lastReceivedAt: null,
        },
      ],
      streams: [
        {
          id: 'logs/default',
          name: 'default',
          streamType: 'logs',
          status: 'healthy',
          rows: 120,
          storedBytes: 2048,
          firstReceivedAt: 100,
          lastReceivedAt: 200,
        },
      ],
    } as HomeOverview;
    expect(
      summarizeSourceSignals(source({ signals: ['logs', 'metrics'] }), overview),
    ).toEqual({
      status: 'healthy',
      rows: 120,
      storedBytes: 2048,
      lastReceivedAt: 200,
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
      lastReceivedAt: null,
      activeSignals: 0,
      expectedSignals: 1,
      streamNames: [],
    });
  });
});

import { describe, expect, it } from 'vitest';

import type { DependencySummary } from '@/api/apm';

import { dependencyScore, slowStatements, targetFigures } from './dependencyHealth';

function call(operation: string, requests: number, errors: number, p95: number, category: DependencySummary['dependency']['category'] = 'database', timeouts = 0): DependencySummary {
  return {
    service: { namespace: 'shop', name: 'checkout', environment: 'prod' },
    dependency: { category, target: 'postgresql', operation },
    red: { requestCount: requests, errorCount: errors, errorRate: errors / requests, durationSum: 0, p95Time: p95, latencyPartial: false, exemplars: [], timeoutCount: timeouts },
    totalTime: 0,
    traces: {} as DependencySummary['traces'],
  };
}

describe('dependency health', () => {
  it('adds the operations of a target up, its P95 the slowest', () => {
    expect(targetFigures([call('a', 10, 1, 100), call('b', 30, 0, 900, 'database', 2)])).toEqual({ requests: 40, errors: 1, timeouts: 2, retries: 0, p95: 900 });
  });

  it('takes points off for failures, timeouts and a slower P95', () => {
    expect(dependencyScore({ requests: 100, errors: 0, timeouts: 0, retries: 0, p95: 100 })).toEqual({ score: 100, level: 'good', reasons: [] });

    const worse = dependencyScore({ requests: 100, errors: 5, timeouts: 2, retries: 0, p95: 200 }, { requests: 100, errors: 0, timeouts: 0, retries: 0, p95: 100 });

    expect(worse.reasons).toEqual([
      { key: 'errors', points: 20 },
      { key: 'timeouts', points: 8 },
      { key: 'latency', points: 20 },
    ]);
    expect(worse).toMatchObject({ score: 52, level: 'poor' });
  });

  it('lists the slowest statements of the databases only', () => {
    const listed = slowStatements([call('SELECT ? FROM a', 10, 0, 500), call('GET', 10, 0, 9_000, 'cache'), call('UPDATE b SET c = ?', 5, 0, 800), call('__other__', 5, 0, 99_000), call('SELECT', 9, 0, 70_000)]);

    expect(listed.map((item) => item.dependency.operation)).toEqual(['UPDATE b SET c = ?', 'SELECT ? FROM a']);
  });
});

import { describe, expect, it } from 'vitest';

import type { SessionRow, WebVitalsPoint } from '@/api/rum';

import {
  ALL,
  applySessionScope,
  initialScope,
  overviewMetrics,
  slowestPages,
} from './model';

function session(
  id: string,
  overrides: Partial<SessionRow> = {},
): SessionRow {
  return {
    session_id: id,
    journey: ['/checkout'],
    rage_click_count: 0,
    dead_click_count: 0,
    slow_resource_count: 0,
    failed_request_count: 0,
    crash_count: 0,
    experience: 'good',
    replay_available: false,
    ...overrides,
  };
}

describe('RUM overview model', () => {
  it('applies application, environment, version, region, and device scope', () => {
    const rows = [
      session('one', {
        application: 'storefront',
        environment: 'prod',
        version: '2.0.0',
        country: 'CN',
        device: 'desktop',
      }),
      session('two', {
        application: 'admin',
        environment: 'prod',
        version: '2.0.0',
        country: 'US',
        device: 'mobile',
      }),
    ];

    expect(
      applySessionScope(rows, {
        ...initialScope(),
        application: 'storefront',
        country: 'CN',
      }).map((row) => row.session_id),
    ).toEqual(['one']);
    expect(
      applySessionScope(rows, {
        application: ALL,
        environment: ALL,
        version: ALL,
        country: ALL,
        device: ALL,
      }),
    ).toHaveLength(2);
  });

  it('computes user, error-free, and Core Web Vitals P75 metrics', () => {
    const sessions = [
      session('one', { user_id: 'u-1' }),
      session('two', {
        user_id: 'u-1',
        error_count: 1,
        experience: 'poor',
      }),
      session('three', { user_id: 'u-2' }),
      session('four'),
    ];
    const vitals: WebVitalsPoint[] = [1_000, 2_000, 3_000, 5_000].map(
      (lcp, index) => ({
        ts_micros: index,
        session_id: sessions[index]!.session_id,
        page: '/checkout',
        lcp_ms: lcp,
        inp_ms: [80, 120, 220, 600][index]!,
        cls: [0.01, 0.05, 0.12, 0.3][index]!,
      }),
    );

    const metrics = overviewMetrics(sessions, vitals);
    expect(metrics.users).toBe(3);
    expect(metrics.sessions).toBe(4);
    expect(metrics.errorFreeRate).toBe(0.75);
    expect(metrics.lcpP75).toBe(3_000);
    expect(metrics.inpP75).toBe(220);
    expect(metrics.clsP75).toBe(0.12);
  });

  it('ranks slow pages and carries their affected-session error rate', () => {
    const sessions = [
      session('one', { last_page: '/checkout', error_count: 1 }),
      session('two', { last_page: '/checkout' }),
      session('three', { last_page: '/catalog', journey: ['/catalog'] }),
    ];
    const vitals: WebVitalsPoint[] = [
      { ts_micros: 1, session_id: 'one', page: '/checkout', lcp_ms: 5_000 },
      { ts_micros: 2, session_id: 'two', page: '/checkout', lcp_ms: 4_000 },
      { ts_micros: 3, session_id: 'three', page: '/catalog', lcp_ms: 1_000 },
    ];

    const pages = slowestPages(vitals, sessions);
    expect(pages[0]).toMatchObject({
      page: '/checkout',
      p75: 5_000,
      sessions: 2,
      errorRate: 0.5,
      grade: 'poor',
    });
  });
});

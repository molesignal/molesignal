import { describe, expect, it } from 'vitest';

import type { RumAlertMetric } from '@/types/alerting';

import { incidentRange, rumOpenPath } from './links';
import type { IncidentFigure } from './samples';

const RANGE = { from: '2026-10-23T08:00:00.000Z', to: '2026-10-23T10:00:00.000Z' };

function figure(metric: RumAlertMetric | null, patch: Partial<IncidentFigure> = {}): IncidentFigure {
  return { application: 'storefront', environment: 'prod', page: null, version: null, metric, ...patch };
}

function open(metric: RumAlertMetric | null, patch: Partial<IncidentFigure> = {}) {
  const url = new URL(rumOpenPath(figure(metric, patch), RANGE), 'https://x.test');

  return { path: url.pathname, params: url.searchParams };
}

describe('where an incident opens in RUM', () => {
  it('is the rows it is read from for what the application measures itself', () => {
    const vital = open('custom_vital', { customName: 'checkout_ready' });

    expect(vital.path).toBe('/rum/explorer');
    expect(vital.params.get('type')).toBe('vitals');
    expect(vital.params.get('q')).toBe("application.id = 'storefront' AND env = 'prod' AND vital.name = 'checkout_ready'");
    expect(vital.params.get('time')).toBe(`${RANGE.from}..${RANGE.to}`);

    const timing = open('custom_timing', { customName: 'cart_ready' });

    expect(timing.params.get('type')).toBe('views');
    expect(timing.params.get('q')).toContain("view.custom_timings.cart_ready != ''");
  });

  it('is the performance page for a vital, of the same application and environment, over the time of the incident', () => {
    const { path, params } = open('lcp_p75');

    expect(path).toBe('/rum/performance');
    expect(params.get('app')).toBe('storefront');
    expect(params.get('env')).toBe('prod');
    expect(params.get('vital')).toBe('lcp');
    expect(params.get('from')).toBe(RANGE.from);
    expect(params.get('to')).toBe(RANGE.to);
    expect(params.get('time')).toBe(`${RANGE.from}..${RANGE.to}`);
    // No page: the table is left alone.
    expect(params.has('open')).toBe(false);
  });

  it('opens the page of the rule in the drawer of the vital', () => {
    const { params } = open('inp_p75', { page: '/checkout' });

    expect(params.get('vital')).toBe('inp');
    expect(params.get('by')).toBe('page');
    expect(params.get('open')).toBe('/checkout');
    expect(params.get('tab')).toBe('inp');
  });

  it('has no drawer for the first byte, which is read beside the vitals', () => {
    const { params } = open('ttfb_p75', { page: '/checkout' });

    expect(params.get('vital')).toBe('ttfb');
    expect(params.get('open')).toBe('/checkout');
    expect(params.has('tab')).toBe(false);
  });

  it('is the resources for failed requests, the errors for errors and issues and crashes, and the frustration page for frustration', () => {
    expect(open('failed_request_rate').path).toBe('/rum/performance/resources');

    for (const metric of ['error_rate', 'error_count', 'new_issues', 'regressed_issues', 'crash_free_sessions'] as const) {
      expect(open(metric).path).toBe('/rum/errors');
    }

    expect(open('frustration_rate').path).toBe('/rum/frustration');
  });

  it('is the explorer on the query of a rule made from it, in its application and environment', () => {
    const search = {
      type: 'resources',
      statement: "resource.url_host = 'api.shop.example' OR resource.status_code >= 500",
      measure: { fn: 'p95', field: 'resource.duration' },
    };
    const query = open('query', { search });

    expect(query.path).toBe('/rum/explorer');
    expect(query.params.get('type')).toBe('resources');
    expect(query.params.get('q')).toBe(
      "application.id = 'storefront' AND env = 'prod' AND (resource.url_host = 'api.shop.example' OR resource.status_code >= 500)",
    );
    expect(query.params.get('viz')).toBe('timeseries');
    expect(query.params.get('m')).toBe('p95:resource.duration');
    expect(query.params.get('time')).toBe(`${RANGE.from}..${RANGE.to}`);
    // Without the rule, there is no query to open.
    expect(open('query').path).toBe('/rum/overview');
  });

  it('is the overview for a figure that no page tells, and for one that is not known', () => {
    expect(open('session_count').path).toBe('/rum/overview');
    expect(open('view_count').path).toBe('/rum/overview');
    expect(open(null).path).toBe('/rum/overview');
  });

  it('names no environment of a rule that watches every one', () => {
    expect(open('error_rate', { environment: null }).params.has('env')).toBe(false);
  });
});

describe('the time of an incident', () => {
  const created = Date.parse('2026-10-23T09:00:00.000Z') * 1_000;

  it('starts before the window the rule read, and ends a little after the resolving', () => {
    const range = incidentRange({ createdAt: created, resolvedAt: Date.parse('2026-10-23T09:30:00.000Z') * 1_000 }, 900);

    // The window of the rule, 15 minutes, and 15 minutes more.
    expect(range.from).toBe('2026-10-23T08:30:00.000Z');
    expect(range.to).toBe('2026-10-23T09:45:00.000Z');
  });

  it('ends now while the incident is going on, and reads an hour back where the rule is not known', () => {
    const now = Date.parse('2026-10-23T09:20:00.000Z');
    const range = incidentRange({ createdAt: created }, undefined, now);

    expect(range.from).toBe('2026-10-23T07:45:00.000Z');
    expect(range.to).toBe('2026-10-23T09:20:00.000Z');
  });
});

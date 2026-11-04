import { describe, expect, it } from 'vitest';

import type { RumAlertPreview } from '@/api/rum';
import type { RumAlertMetric } from '@/types/alerting';

import {
  emptyRumDraft,
  emptySearch,
  fitted,
  isAboutIssues,
  RUM_ANOMALY_METRICS,
  RUM_METRICS,
  rumDefaultMinSamples,
  rumDefaultThreshold,
  rumDefaultWindow,
  rumDraftFromSearch,
  rumMetricUnit,
  rumNewAnomalyHref,
  rumNewRuleHref,
  rumPreviewResult,
  rumRuleLabels,
  rumRuleQuery,
  rumSupportsPage,
  rumSupportsVersion,
  searchAddsUp,
  searchUnit,
} from './rumSource';

describe('RUM alert source', () => {
  it('lists each figure once, and keeps new and regressed issues out of the anomaly rules', () => {
    expect(new Set(RUM_METRICS).size).toBe(17);
    expect(RUM_ANOMALY_METRICS).toHaveLength(15);
    expect(RUM_ANOMALY_METRICS.some(isAboutIssues)).toBe(false);
  });

  it('says in which unit a figure is and what it needs, as the server does', () => {
    const units = Object.fromEntries(RUM_METRICS.map((metric) => [metric, rumMetricUnit(metric)]));

    expect(units).toMatchObject({
      session_count: 'count',
      error_rate: 'percent',
      crash_free_sessions: 'percent',
      lcp_p75: 'milliseconds',
      cls_p75: 'score',
      new_issues: 'count',
    });
    // A count needs no rows; a share or a percentile does.
    expect(RUM_METRICS.filter((metric) => rumDefaultMinSamples(metric) === 0)).toEqual(['error_count', 'new_issues', 'regressed_issues', 'issue_impact', 'session_count', 'view_count']);
    expect(rumDefaultMinSamples('failed_request_rate')).toBe(200);
    expect(rumDefaultMinSamples('lcp_p75')).toBe(50);
  });

  it('narrows a figure to a page or a version only where it can be told by one', () => {
    expect(rumSupportsPage('lcp_p75')).toBe(true);
    expect(rumSupportsPage('error_count')).toBe(true);
    expect(rumSupportsPage('crash_free_sessions')).toBe(false);
    expect(rumSupportsPage('new_issues')).toBe(false);
    expect(rumSupportsVersion('crash_free_sessions')).toBe(true);
    expect(rumSupportsVersion('regressed_issues')).toBe(false);
  });

  it('starts every figure on a line people commonly page on, in its own unit', () => {
    expect(rumDefaultThreshold('lcp_p75')).toEqual({ operator: 'gt', threshold: 2_500 });
    expect(rumDefaultThreshold('crash_free_sessions')).toEqual({ operator: 'lt', threshold: 99.5 });
    expect(rumDefaultThreshold('session_count')).toEqual({ operator: 'lt', threshold: 1 });

    for (const metric of RUM_METRICS) {
      expect(Number.isFinite(rumDefaultThreshold(metric).threshold)).toBe(true);
    }
  });

  it('watches the figures that need many rows over a longer window', () => {
    expect(rumDefaultWindow('lcp_p75')).toBe(3_600);
    expect(rumDefaultWindow('error_rate')).toBe(900);
  });

  it('opens on what a RUM page asked for, and on nothing else', () => {
    const href = rumNewRuleHref({ application: 'shop-web', environment: 'prod', page: '/checkout', version: '2.4.1', metric: 'lcp_p75' });

    expect(rumDraftFromSearch(href.slice(href.indexOf('?')))).toEqual({
      applicationId: 'shop-web',
      environment: 'prod',
      page: '/checkout',
      version: '2.4.1',
      metric: 'lcp_p75',
      minSamples: null,
    });
    // A figure the server does not know, and a page a figure cannot be narrowed to, are left out.
    expect(rumDraftFromSearch('?source=rum&app=shop-web&metric=nope&page=/x&environment=')).toMatchObject({
      metric: 'error_rate',
      environment: null,
      page: '/x',
    });
    expect(rumDraftFromSearch('?source=rum&app=shop-web&metric=crash_free_sessions&page=/x')).toMatchObject({ page: null });
    expect(rumDraftFromSearch('?app=shop-web')).toBeNull();
    expect(rumNewAnomalyHref({ application: 'shop-web' })).toBe('/alerts/anomaly/add?source=rum&app=shop-web&metric=error_rate');
  });

  it('offers an anomaly rule fewer figures than a threshold rule', () => {
    expect(rumDraftFromSearch('?source=rum&app=a&metric=new_issues', RUM_ANOMALY_METRICS)).toMatchObject({ metric: 'error_rate' });
    expect(rumDraftFromSearch('?source=rum&app=a&metric=new_issues')).toMatchObject({ metric: 'new_issues' });
  });

  it('drops the narrowing a figure cannot have, and the minimum of rows of a count', () => {
    expect(fitted({ ...emptyRumDraft('new_issues'), applicationId: 'a', page: '/x', version: '1', minSamples: 5 })).toMatchObject({
      page: null,
      version: null,
      minSamples: null,
    });
    expect(fitted({ ...emptyRumDraft('error_rate'), applicationId: 'a', page: '/x', minSamples: 5 })).toMatchObject({ page: '/x', minSamples: 5 });
  });

  it('saves a rule with no stream and labels its incidents with the application', () => {
    const draft = { ...emptyRumDraft('lcp_p75'), applicationId: ' shop-web ', environment: 'prod', page: '/checkout' };

    expect(rumRuleQuery(draft, 3_600)).toEqual({
      language: 'sql',
      statement: '',
      periodSecs: 3_600,
      stream: null,
      rum: { applicationId: 'shop-web', environment: 'prod', page: '/checkout', version: null, metric: 'lcp_p75', minSamples: null },
    });
    expect(rumRuleLabels({ ...draft, applicationId: 'shop-web' })).toEqual({
      'rum.application': 'shop-web',
      'deployment.environment': 'prod',
      'rum.page': '/checkout',
    });
  });

  it('opens a rule made from the explorer on its query, and keeps the query for that figure alone', () => {
    const search = {
      type: 'resources',
      statement: "resource.url_host = 'api.shop.example'",
      filters: [{ field: 'resource.url_host', op: '=', value: 'api.shop.example' }],
      freeText: ['checkout'],
      measure: { fn: 'p95', field: 'resource.duration' },
    };
    const href = rumNewRuleHref({ application: 'shop-web', environment: 'prod', metric: 'query', search });
    const draft = rumDraftFromSearch(href.slice(href.indexOf('?')));

    expect(draft).toEqual({
      applicationId: 'shop-web',
      environment: 'prod',
      page: null,
      version: null,
      metric: 'query',
      minSamples: null,
      search,
    });
    // A query names the page and the version among its filters.
    expect(rumSupportsPage('query')).toBe(false);
    expect(rumSupportsVersion('query')).toBe(false);
    // Another figure has no query, and a query of nothing is a count of every view.
    expect(fitted({ ...draft!, metric: 'error_rate' })).not.toHaveProperty('search');
    expect(fitted({ ...emptyRumDraft('query'), applicationId: 'a' }).search).toEqual(emptySearch());
    expect(rumDraftFromSearch('?source=rum&app=a&metric=query&filters=nope&measure=count')?.search).toEqual({
      ...emptySearch(),
      statement: '',
    });
  });

  it('needs no minimum of rows for a count or a sum of a query, and says its unit by its field', () => {
    const count = { ...emptySearch(), measure: { fn: 'count' } };
    const slow = { ...emptySearch(), measure: { fn: 'p75', field: 'view.loading_time' } };

    expect(searchAddsUp(count)).toBe(true);
    expect(searchAddsUp({ ...emptySearch(), measure: { fn: 'sum', field: 'view.error.count' } })).toBe(true);
    expect(searchAddsUp(slow)).toBe(false);
    expect(fitted({ ...emptyRumDraft('query'), applicationId: 'a', minSamples: 10, search: count }).minSamples).toBeNull();
    expect(fitted({ ...emptyRumDraft('query'), applicationId: 'a', minSamples: 10, search: slow }).minSamples).toBe(10);
    expect(searchUnit(count, 'ns')).toBe('count');
    expect(searchUnit(slow, 'ns')).toBe('milliseconds');
    expect(searchUnit(slow, 'bytes')).toBe('bytes');
    expect(searchUnit(slow, 'px')).toBe('none');
  });

  it('reads a preview as the rows of a query, a window with no value having none', () => {
    const preview: RumAlertPreview = {
      metric: 'lcp_p75' as RumAlertMetric,
      unit: 'milliseconds',
      minSamples: 50,
      periodSecs: 900,
      from: 0,
      to: 3,
      points: [
        { at: 0, value: null, samples: 10 },
        { at: 1, value: 2_400, samples: 80 },
        { at: 2, value: 3_100, samples: 90 },
      ],
      current: { at: 2, value: 3_100, samples: 90 },
    };

    expect(rumPreviewResult(preview, 7)).toEqual({
      columns: ['_timestamp', 'value'],
      rows: [
        [1, 2_400],
        [2, 3_100],
      ],
      scannedRows: 2,
      tookMs: 7,
    });
  });
});

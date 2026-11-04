import { describe, expect, it } from 'vitest';

import { rumDraftFromSearch } from '@/routes/alerts/rule-workbench/rumSource';

import { resolveVizSpec } from '../viz/spec';
import { rumAlertHref, rumAlertPlan } from './alertSource';

const FIELDS = [
  { name: 'browser.name', dataType: 'utf8', unit: 'none', facetable: true, indexed: false },
  { name: 'geo.country_iso_code', dataType: 'utf8', unit: 'none', facetable: true, indexed: false },
  { name: 'view.loading_time', dataType: 'int64', unit: 'ns', facetable: false, indexed: false },
] as const;

const address = {
  measures: ['p75:view.loading_time', 'count'],
  groups: ['browser.name'],
  bucket: null,
  top: 5,
  sort: null,
  descending: null,
  field: 'view.loading_time',
};

const statement = "application.id = 'shop-web' and env = 'prod' and browser.name != 'Safari'";
const filters = [
  { field: 'application.id', op: '=' as const, value: 'shop-web' },
  { field: 'env', op: '=' as const, value: 'prod' },
  { field: 'browser.name', op: '!=' as const, value: 'Safari' },
];

describe('the alert rule of what the RUM explorer draws', () => {
  it('takes the query and its first number, and says what a rule cannot watch', () => {
    const plan = rumAlertPlan({
      tab: 'views',
      viz: 'table',
      spec: resolveVizSpec('table', address, 'views', FIELDS),
      statement: ` ${statement} `,
      filters,
      freeText: ['checkout'],
      applications: ['shop-web', 'shop-app'],
    });

    expect(plan.search).toEqual({
      type: 'views',
      statement,
      filters,
      freeText: ['checkout'],
      measure: { fn: 'p75', field: 'view.loading_time' },
    });
    // The query names the application and the environment: the rule is about them.
    expect(plan.application).toBe('shop-web');
    expect(plan.environment).toBe('prod');
    expect(plan.viz).toBe('table');
    expect(plan.groups).toEqual(['browser.name']);
    expect(plan.measures).toEqual([{ fn: 'count' }]);
    expect(plan.distribution).toBe(false);
  });

  it('counts the rows of the list, and watches a distribution by the 75th percentile of its field', () => {
    const list = rumAlertPlan({ tab: 'errors', viz: 'list', spec: null, statement: '', filters: [], freeText: [], applications: ['only'] });

    expect(list.search.measure).toEqual({ fn: 'count' });
    expect(list).toMatchObject({ viz: null, groups: [], measures: [], application: 'only', environment: null });

    const spread = rumAlertPlan({
      tab: 'views',
      viz: 'distribution',
      spec: resolveVizSpec('distribution', address, 'views', FIELDS),
      statement: '',
      filters: [],
      freeText: [],
      applications: ['a', 'b'],
    });

    expect(spread.search.measure).toEqual({ fn: 'p75', field: 'view.loading_time' });
    expect(spread).toMatchObject({ viz: 'distribution', distribution: true, groups: [], application: '' });

    const map = rumAlertPlan({
      tab: 'views',
      viz: 'map',
      spec: resolveVizSpec('map', address, 'views', FIELDS),
      statement: '',
      filters: [],
      freeText: [],
      applications: [],
    });

    expect(map.groups).toEqual(['geo.country_iso_code']);
  });

  it('leaves the application to the editor when the query names several or none', () => {
    const several = rumAlertPlan({
      tab: 'views',
      viz: 'list',
      spec: null,
      statement: '',
      filters: [
        { field: 'application.id', op: 'in', value: '["a","b"]' },
        { field: 'env', op: '!=', value: 'staging' },
      ],
      freeText: [],
      applications: ['a', 'b'],
    });

    expect(several.application).toBe('');
    expect(several.environment).toBeNull();
  });

  it('opens the editor on the query, against a threshold or against the days before', () => {
    const plan = rumAlertPlan({
      tab: 'views',
      viz: 'timeseries',
      spec: resolveVizSpec('timeseries', address, 'views', FIELDS),
      statement,
      filters,
      freeText: [],
      applications: [],
    });

    const threshold = rumAlertHref(plan, 'threshold');
    const anomaly = rumAlertHref(plan, 'anomaly');

    expect(threshold.startsWith('/alerts/rules/new?source=rum&app=shop-web&metric=query')).toBe(true);
    expect(anomaly.startsWith('/alerts/anomaly/add?')).toBe(true);
    expect(rumDraftFromSearch(threshold.slice(threshold.indexOf('?')))).toMatchObject({
      applicationId: 'shop-web',
      environment: 'prod',
      metric: 'query',
      search: { ...plan.search, freeText: [] },
    });
  });
});

import type { TFunction } from 'i18next';
import { describe, expect, it } from 'vitest';

import type { RumPanelStatement } from '@/api/rum';
import { createEmptyDashboardDefinition, dashboardDefinitionFromStoredModel } from '@/dashboard-engine/model';

import { resolveVizSpec } from '../viz/spec';
import {
  INTERVALS,
  intervalFor,
  intervalVariable,
  MAP_COUNTRIES,
  nearestInterval,
  rumAddedDashboardModel,
  rumDashboardPanel,
  rumNewDashboardModel,
  rumPanelPlan,
} from './panelModel';

const t = ((key: string, options?: Record<string, unknown>) =>
  options ? `${key}(${Object.values(options).join(',')})` : key) as unknown as TFunction<'rum'>;

const FIELDS = [
  { name: 'browser.name', dataType: 'utf8', unit: 'none', facetable: true, indexed: false },
  { name: 'view.url_path_group', dataType: 'utf8', unit: 'none', facetable: true, indexed: false },
  { name: 'geo.country_iso_code', dataType: 'utf8', unit: 'none', facetable: true, indexed: false },
  { name: 'view.loading_time', dataType: 'int64', unit: 'ns', facetable: false, indexed: false },
] as const;

const address = {
  measures: ['p75:view.loading_time'],
  groups: ['browser.name'],
  bucket: null,
  top: 5,
  sort: null,
  descending: null,
  field: 'view.loading_time',
};

const filters = [{ field: 'env', op: '=' as const, value: 'prod' }];

const HOUR = 3_600_000_000;

function statement(columns: RumPanelStatement['columns']): RumPanelStatement {
  return { stream: 'rum_views', statement: 'SELECT 1', columns, limit: 10_000 };
}

describe('the panel of what the RUM explorer draws', () => {
  it('asks for the chart with the filters, words, measures, groups and number of groups of the explorer', () => {
    const plan = rumPanelPlan({
      t,
      tab: 'views',
      viz: 'timeseries',
      spec: resolveVizSpec('timeseries', address, 'views', FIELDS),
      filters,
      freeText: ['checkout'],
    });

    expect(plan.chart).toBe('timeseries');
    expect(plan.change).toBeNull();
    expect(plan.additive).toBe(false);
    expect(plan.params).toEqual({
      type: 'views',
      filters,
      freeText: ['checkout'],
      chart: 'timeseries',
      measures: [{ fn: 'p75', field: 'view.loading_time', name: 'p75 explorer.columns.loading_time' }],
      groupBy: ['browser.name'],
      groupNames: ['explorer.columns.browser'],
      limit: 5,
    });
  });

  it('draws the list as the count its histogram draws, and a map as the countries it ranks first', () => {
    const list = rumPanelPlan({ t, tab: 'errors', viz: 'list', spec: null, filters: [], freeText: [] });

    expect(list).toMatchObject({ chart: 'timeseries', change: 'list_as_count', additive: true });
    expect(list.params).toEqual({ type: 'errors', chart: 'timeseries', measures: [{ fn: 'count', name: 'explorer.viz.count_of(explorer.types.errors)' }] });

    const map = rumPanelPlan({ t, tab: 'views', viz: 'map', spec: resolveVizSpec('map', address, 'views', FIELDS), filters: [], freeText: [] });

    expect(map).toMatchObject({ chart: 'toplist', change: 'map_as_top' });
    expect(map.params).toMatchObject({ groupBy: ['geo.country_iso_code'], limit: MAP_COUNTRIES });
  });

  it('orders a table by the column the reader chose, as the panel names it', () => {
    const spec = resolveVizSpec(
      'table',
      { ...address, measures: ['count', 'p75:view.loading_time'], sort: 'p75(view.loading_time)', descending: false },
      'views',
      FIELDS,
    );
    const plan = rumPanelPlan({ t, tab: 'views', viz: 'table', spec, filters: [], freeText: [] });

    expect(plan.params).toMatchObject({ chart: 'table', orderBy: 'p75 explorer.columns.loading_time', order: 'asc' });
  });

  it('draws a distribution of the field the explorer draws, named by the field', () => {
    const plan = rumPanelPlan({
      t,
      tab: 'views',
      viz: 'distribution',
      spec: resolveVizSpec('distribution', address, 'views', FIELDS),
      filters: [],
      freeText: [],
    });

    expect(plan.params).toMatchObject({ chart: 'distribution', field: 'view.loading_time', groupNames: ['explorer.columns.loading_time'] });
    expect(plan.title).toBe('explorer.export.dashboard.distribution_title(explorer.columns.loading_time)');
  });

  it('cuts a chart over time into the interval nearest the bars of the explorer', () => {
    expect(nearestInterval({ from: 0, to: HOUR }, null).value).toBe('1 minute');
    expect(nearestInterval({ from: 0, to: 24 * HOUR }, null).value).toBe('1 hour');
    expect(nearestInterval({ from: 0, to: HOUR }, 15 * 60_000_000).value).toBe('15 minutes');

    const variable = intervalVariable(t, INTERVALS[1]);

    expect(variable).toMatchObject({ name: 'interval', type: 'interval', currentValue: '5 minutes', defaultValue: '5 minutes' });
    expect(variable.options?.filter((option) => option.selected).map((option) => option.value)).toEqual(['5 minutes']);
  });

  it('uses the interval a dashboard has, adds it where there is none, and keeps one of the same name that is something else', () => {
    const fixed = INTERVALS[2];
    const none = createEmptyDashboardDefinition('a');

    expect(intervalFor(none, fixed)).toEqual({ interval: '${interval}', adds: true });
    expect(intervalFor({ variables: [intervalVariable(t, fixed)] }, fixed)).toEqual({ interval: '${interval}', adds: false });
    expect(intervalFor({ variables: [{ ...intervalVariable(t, fixed), type: 'custom' }] }, fixed)).toEqual({
      interval: '15 minutes',
      adds: false,
    });
  });

  it('draws a chart over time with a line for each group, and a count stacked in bars', () => {
    const columns: RumPanelStatement['columns'] = [
      { name: 'time', role: 'time', unit: 'none' },
      { name: 'Browser', role: 'group', unit: 'none' },
      { name: 'p75', role: 'measure', unit: 'ms' },
    ];
    const lines = rumDashboardPanel(statement(columns), { chart: 'timeseries', additive: false }, 'Load time');

    expect(lines.visualization.type).toBe('time_series');
    expect(lines.queries[0]).toMatchObject({
      dataSourceType: 'sql',
      format: 'time_series',
      query: { language: 'sql', statement: 'SELECT 1', streamName: 'rum_views', streamType: 'logs', limit: 10_000 },
    });
    expect(lines.visualization.options).toMatchObject({ drawStyle: 'line', legendMode: 'list' });
    expect(lines.fieldConfig).toEqual({ unit: 'milliseconds', decimals: 1, softMin: 0 });

    const bars = rumDashboardPanel(statement(columns), { chart: 'timeseries', additive: true }, 'Views');

    expect(bars.visualization.options).toMatchObject({ drawStyle: 'bar', stackMode: 'normal' });
  });

  it('draws a top list across, a distribution up, and a table with each measure in its unit', () => {
    const ranked: RumPanelStatement['columns'] = [
      { name: 'Page', role: 'group', unit: 'none' },
      { name: 'Views', role: 'measure', unit: 'count' },
      { name: 'Size', role: 'measure', unit: 'bytes' },
    ];

    const top = rumDashboardPanel(statement(ranked.slice(0, 2)), { chart: 'toplist', additive: true }, 'Pages');

    expect(top.visualization).toMatchObject({ type: 'bar_chart', options: { orientation: 'horizontal' } });
    expect(top.queries[0]).not.toHaveProperty('format');
    expect(top.fieldConfig).toMatchObject({ unit: 'short', decimals: 0, color: { mode: 'fixed' } });

    const table = rumDashboardPanel(statement(ranked), { chart: 'table', additive: true }, 'Pages');

    expect(table.visualization.type).toBe('table');
    expect(table.fieldConfig).toEqual({});
    expect(table.overrides.map((override) => [override.matcher.value, override.properties])).toEqual([
      ['Views', [{ id: 'unit', value: 'short' }, { id: 'decimals', value: 0 }]],
      ['Size', [{ id: 'unit', value: 'bytes' }, { id: 'decimals', value: 0 }]],
    ]);

    const spread = rumDashboardPanel(statement(ranked.slice(0, 2)), { chart: 'distribution', additive: true }, 'Load time');

    expect(spread.visualization).toMatchObject({ type: 'bar_chart', options: { orientation: 'vertical' } });
  });

  it('makes a new dashboard of the panel alone, or adds it under what a dashboard has', () => {
    const columns: RumPanelStatement['columns'] = [{ name: 'Views', role: 'measure', unit: 'count' }];
    const panel = rumDashboardPanel(statement(columns), { chart: 'table', additive: true }, 'Views');
    const window = { mode: 'relative' as const, from: 'now-6h', to: 'now' };

    const created = dashboardDefinitionFromStoredModel(rumNewDashboardModel('RUM', panel, window, intervalVariable(t, INTERVALS[1])), 'x', 'x');

    expect(created).toMatchObject({ title: 'RUM', tags: ['rum'], timeSettings: { defaultFrom: 'now-6h', defaultTo: 'now' } });
    expect(created.variables.map((variable) => variable.name)).toEqual(['interval']);
    expect(created.elements).toHaveLength(1);

    const existing = { ...createEmptyDashboardDefinition('Team'), elements: [panel] };
    const below = rumDashboardPanel(statement(columns), { chart: 'table', additive: true }, 'Again', existing.elements);
    const added = dashboardDefinitionFromStoredModel(rumAddedDashboardModel(existing, below, null), 'x', 'x');

    expect(added.title).toBe('Team');
    expect(added.variables).toEqual([]);
    expect(added.elements.map((element) => element.title)).toEqual(['Views', 'Again']);
    // The new panel sits under the ones there were.
    expect(added.elements[1]!.gridPos.y).toBe(panel.gridPos.y + panel.gridPos.h);
  });
});

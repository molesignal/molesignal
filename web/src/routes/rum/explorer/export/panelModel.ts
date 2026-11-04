import type { TFunction } from 'i18next';

import type { RumFilter, RumMeasure, RumPanelChart, RumPanelColumn, RumPanelStatement, RumPanelStatementParams } from '@/api/rum';
import { createDashboardPanel } from '@/dashboard-engine/factories';
import { createEmptyDashboardDefinition, dashboardDefinitionToModel } from '@/dashboard-engine/model';
import type {
  DashboardDefinition,
  DashboardElement,
  DashboardPanel,
  DashboardVariable,
  FieldConfig,
  FieldOverride,
  VisualizationType,
} from '@/dashboard-engine/schema';
import type { TimeWindow } from '@/stores/useTimeStore';

import { columnLabel } from '../labels';
import type { RumTab } from '../model';
import type { RumViz } from '../viz/address';
import { measureLabel, vizTitle } from '../viz/labels';
import { autoBucketOf, COUNT, COUNTRY_FIELD, isAdditive, measureName, type RumMeasureChoice, type RumVizSpec } from '../viz/spec';

/** How many countries a map becomes on a dashboard, which has no map: the ones it ranks first. */
export const MAP_COUNTRIES = 20;

/** The longest name the server gives a column of a panel. */
const MAX_NAME = 64;

/** What changes on the way to a dashboard; nothing for a chart a dashboard draws as it is. */
export type RumPanelChange = 'list_as_count' | 'map_as_top' | null;

/** What the explorer draws, as a panel of a dashboard draws it. */
export interface RumPanelPlan {
  chart: RumPanelChart;
  /** The statement to ask for, but for the interval, which the dashboard it goes to decides. */
  params: RumPanelStatementParams;
  change: RumPanelChange;
  /** What the panel is called unless the reader says otherwise: the line over the drawing. */
  title: string;
  /** Whether the numbers are counts or sums, which a chart over time stacks in bars. */
  additive: boolean;
}

interface PlanInput {
  t: TFunction<'rum'>;
  tab: RumTab;
  viz: RumViz;
  /** What is drawn; none while the list is in front. */
  spec: RumVizSpec | null;
  filters: readonly RumFilter[];
  freeText: readonly string[];
}

const cut = (name: string) => [...name].slice(0, MAX_NAME).join('');

/**
 * The panel of what the explorer draws: its filters and words, its measures, groups, order and
 * number of groups, named as the explorer names them. The list is the count over time its
 * histogram draws, and a map, which a dashboard does not have, the countries it ranks first.
 */
export function rumPanelPlan({ t, tab, viz, spec, filters, freeText }: PlanInput): RumPanelPlan {
  const query = {
    type: tab,
    ...(filters.length > 0 ? { filters: [...filters] } : {}),
    ...(freeText.length > 0 ? { freeText: [...freeText] } : {}),
  };
  const label = (measure: RumMeasureChoice) => cut(measureLabel(t, measure, tab));
  const measureOf = (measure: RumMeasureChoice): RumMeasure => ({
    fn: measure.fn,
    ...(measure.field ? { field: measure.field } : {}),
    name: label(measure),
  });

  if (viz === 'list' || !spec) {
    return {
      chart: 'timeseries',
      change: 'list_as_count',
      title: label(COUNT),
      additive: true,
      params: { ...query, chart: 'timeseries', measures: [measureOf(COUNT)] },
    };
  }

  if (spec.viz === 'distribution') {
    const field = spec.field ?? '';
    const name = cut(columnLabel(t, field));

    return {
      chart: 'distribution',
      change: null,
      title: t('explorer.export.dashboard.distribution_title', { field: name }),
      additive: true,
      params: { ...query, chart: 'distribution', field, measures: [measureOf(COUNT)], groupNames: [name] },
    };
  }

  const map = spec.viz === 'map';
  const chart: RumPanelChart = spec.viz === 'map' || spec.viz === 'toplist' ? 'toplist' : spec.viz;
  const groups = map ? [COUNTRY_FIELD] : spec.groups;
  const measures = spec.measures.map(measureOf);
  // A table is ordered by a measure as the server calls it, which is the name given here.
  const sortedBy = spec.sort
    ? (measures[spec.measures.findIndex((measure) => measureName(measure) === spec.sort?.by)]?.name ?? spec.sort.by)
    : null;

  return {
    chart,
    change: map ? 'map_as_top' : null,
    title: vizTitle(t, label(spec.measures[0] ?? COUNT), groups),
    additive: isAdditive(spec.measures[0]?.fn ?? 'count'),
    params: {
      ...query,
      chart,
      measures,
      ...(groups.length > 0 ? { groupBy: groups, groupNames: groups.map((group) => cut(columnLabel(t, group))) } : {}),
      ...(sortedBy && spec.sort ? { orderBy: sortedBy, order: spec.sort.descending ? ('desc' as const) : ('asc' as const) } : {}),
      limit: map ? MAP_COUNTRIES : spec.top,
    },
  };
}

/** The lengths a chart over time can be cut into, as a dashboard variable offers them. */
export const INTERVALS = [
  { value: '1 minute', key: '1m', micros: 60_000_000 },
  { value: '5 minutes', key: '5m', micros: 300_000_000 },
  { value: '15 minutes', key: '15m', micros: 900_000_000 },
  { value: '1 hour', key: '1h', micros: 3_600_000_000 },
  { value: '1 day', key: '1d', micros: 86_400_000_000 },
] as const;

/** The variable a chart over time is cut by. */
export const INTERVAL_VARIABLE = 'interval';

export type RumInterval = (typeof INTERVALS)[number];

/** The interval of the choices nearest the bars the explorer draws over its range. */
export function nearestInterval(range: { from: number; to: number }, bucket: number | null): RumInterval {
  const bar = bucket ?? autoBucketOf({ start: range.from, end: range.to });

  return INTERVALS.reduce<RumInterval>(
    (best, option) => (Math.abs(Math.log(option.micros / bar)) < Math.abs(Math.log(best.micros / bar)) ? option : best),
    INTERVALS[0],
  );
}

/** The dashboard variable a chart over time is cut by, opening on `current`. */
export function intervalVariable(t: TFunction<'rum'>, current: RumInterval): DashboardVariable {
  return {
    id: 'variable-interval',
    name: INTERVAL_VARIABLE,
    label: t('explorer.export.dashboard.interval_label'),
    type: 'interval',
    options: INTERVALS.map((option) => ({
      label: t(`explorer.export.dashboard.intervals.${option.key}`),
      value: option.value,
      ...(option.value === current.value ? { selected: true } : {}),
    })),
    defaultValue: current.value,
    currentValue: current.value,
    multi: false,
    includeAll: false,
    hide: 'none',
    refresh: 'never',
  };
}

/**
 * How a dashboard cuts a chart over time: by its interval variable, which it is given when it has
 * none, or by `fixed` when a variable of that name is something else.
 */
export function intervalFor(
  definition: Pick<DashboardDefinition, 'variables'> | null,
  fixed: RumInterval,
): { interval: string; adds: boolean } {
  const named = definition?.variables.find((variable) => variable.name === INTERVAL_VARIABLE);
  const variable = `\${${INTERVAL_VARIABLE}}`;

  if (!named) return { interval: variable, adds: true };

  return named.type === 'interval' ? { interval: variable, adds: false } : { interval: fixed.value, adds: false };
}

/** A dashboard unit for the unit of a column, and the decimals a number keeps. */
function unitOf(unit: RumPanelColumn['unit']): Pick<FieldConfig, 'unit' | 'decimals'> {
  switch (unit) {
    case 'ms':
      return { unit: 'milliseconds', decimals: 1 };
    case 'count':
      return { unit: 'short', decimals: 0 };
    case 'bytes':
      return { unit: 'bytes', decimals: 0 };
    case 'percent':
      return { unit: 'percent', decimals: 1 };
    case 'score':
      return { unit: 'none', decimals: 3 };
    case 'px':
      return { unit: 'px', decimals: 0 };
    default:
      return { unit: 'none', decimals: 2 };
  }
}

const VISUALIZATION: Record<RumPanelChart, VisualizationType> = {
  timeseries: 'time_series',
  toplist: 'bar_chart',
  table: 'table',
  distribution: 'bar_chart',
};

/** The colour one measure is drawn in, the first of every chart of the product. */
const FIRST_COLOR = 'var(--chart-1)';

/**
 * The panel of a statement, under the elements a dashboard has: a chart over time with a line for
 * each group, or bars for a count or a sum as the explorer stacks them; a top list in bars across;
 * a table with each measure in its unit; a distribution in bars up.
 */
export function rumDashboardPanel(
  statement: RumPanelStatement,
  plan: Pick<RumPanelPlan, 'chart' | 'additive'>,
  title: string,
  elements: readonly DashboardElement[] = [],
): DashboardPanel {
  const { chart, additive } = plan;
  const panel = createDashboardPanel(elements, VISUALIZATION[chart]);
  const measures = statement.columns.filter((column) => column.role === 'measure');
  const grouped = statement.columns.some((column) => column.role === 'group');
  const only = measures.length === 1 ? measures[0] : undefined;
  const overTime = chart === 'timeseries';

  const options: Record<string, unknown> = overTime
    ? {
        drawStyle: additive ? 'bar' : 'line',
        stackMode: additive && grouped ? 'normal' : 'none',
        fillOpacity: additive ? 0.8 : 0,
        showPoints: 'never',
        legendMode: grouped ? 'list' : 'hidden',
        legendStats: [],
      }
    : chart === 'toplist'
      ? { orientation: 'horizontal', showValues: 'always' }
      : chart === 'distribution'
        ? { orientation: 'vertical', groupWidth: 0.9, showValues: 'never' }
        : { pageSize: 50 };

  const overrides: FieldOverride[] =
    chart === 'table'
      ? measures.map((column, index) => ({
          id: `${panel.id}-${index + 1}`,
          matcher: { type: 'field_name' as const, value: column.name },
          properties: Object.entries(unitOf(column.unit)).map(([id, value]) => ({ id, value })),
        }))
      : [];

  return {
    ...panel,
    title,
    gridPos: { ...panel.gridPos, w: 24, h: chart === 'table' ? 32 : 28 },
    queries: [
      {
        refId: 'A',
        enabled: true,
        dataSourceType: 'sql',
        query: {
          language: 'sql',
          statement: statement.statement,
          streamName: statement.stream,
          streamType: 'logs',
          limit: statement.limit,
        },
        // A row for each bar and group: the dashboard draws a line for each group.
        ...(overTime ? { format: 'time_series' } : {}),
      },
    ],
    visualization: { ...panel.visualization, options: { ...panel.visualization.options, ...options } },
    fieldConfig: {
      ...(only && chart !== 'table' ? unitOf(only.unit) : {}),
      // One measure, ungrouped, is drawn in the first colour; the groups of a chart each have their own.
      ...(chart !== 'table' && !(overTime && grouped) ? { color: { mode: 'fixed' as const, value: FIRST_COLOR } } : {}),
      ...(overTime ? { softMin: 0 } : {}),
    },
    overrides,
  };
}

/**
 * A new dashboard that holds the panel alone. Where the explorer looks at a stretch of time that
 * moves with the clock, the dashboard opens on the same stretch; a chart over time comes with the
 * interval it is cut by.
 */
export function rumNewDashboardModel(
  title: string,
  panel: DashboardPanel,
  window: TimeWindow,
  interval: DashboardVariable | null,
): Record<string, unknown> {
  const definition = createEmptyDashboardDefinition(title);

  return dashboardDefinitionToModel({
    ...definition,
    tags: ['rum'],
    timeSettings:
      window.mode === 'relative' && typeof window.from === 'string' && typeof window.to === 'string'
        ? { ...definition.timeSettings, defaultFrom: window.from, defaultTo: window.to }
        : definition.timeSettings,
    variables: interval ? [interval] : [],
    elements: [panel],
  });
}

/** A dashboard with the panel added under what it has, and the interval variable when it needs one. */
export function rumAddedDashboardModel(
  definition: DashboardDefinition,
  panel: DashboardPanel,
  interval: DashboardVariable | null,
): Record<string, unknown> {
  return dashboardDefinitionToModel({
    ...definition,
    variables: interval ? [...definition.variables, interval] : definition.variables,
    elements: [...definition.elements, panel],
  });
}

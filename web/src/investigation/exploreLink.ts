import type {
  DashboardTimeRange,
  PanelQuery,
} from '@/dashboard-engine/schema';
import {
  interpolateVariables,
  type DashboardVariableValues,
} from '@/dashboard-engine/variables';

export interface PanelExploreLinkOptions {
  query: PanelQuery;
  variables: DashboardVariableValues;
  timeRange: DashboardTimeRange;
}

/**
 * Serialize a Dashboard panel query into the URL contract understood by the
 * matching Explore surface. Keep this as the only Dashboard → Explore bridge:
 * generic `query=` links silently put SQL into the field-query editors.
 */
export function buildPanelExploreLink({
  query,
  variables,
  timeRange,
}: PanelExploreLinkOptions): string {
  const config = query.query;
  const expression = interpolateVariables(
    panelQueryExpression(query),
    variables,
  ).trim();
  const target = exploreTarget(query);
  const params = new URLSearchParams();

  if (expression) params.set(target.queryParam, expression);
  appendTimeRange(params, timeRange);

  for (const [name, value] of Object.entries(variables)) {
    params.set(
      `var-${name}`,
      Array.isArray(value) ? value.join(',') : String(value ?? ''),
    );
  }

  const stream = firstString(config.streamName, config.stream);
  const streamType = firstString(config.streamType, config.stream_type);
  if (stream) params.set('stream', stream);
  if (streamType) params.set('stream_type', streamType);
  if (query.dataSourceId) params.set('data_source', query.dataSourceId);

  const search = params.toString();
  return search ? `${target.pathname}?${search}` : target.pathname;
}

export function panelQueryExpression(query: PanelQuery): string {
  for (const key of ['expression', 'statement', 'sql', 'query']) {
    const value = query.query[key];
    if (typeof value === 'string' && value.trim()) return value;
  }
  return '';
}

function exploreTarget(query: PanelQuery): {
  pathname: string;
  queryParam: 'promql' | 'sql' | 'q';
} {
  if (query.dataSourceType === 'metrics') {
    return { pathname: '/metrics', queryParam: 'promql' };
  }
  if (query.dataSourceType === 'logs') {
    return { pathname: '/logs', queryParam: 'sql' };
  }
  if (query.dataSourceType === 'traces') {
    return { pathname: '/traces', queryParam: queryLanguage(query) === 'sql' ? 'sql' : 'q' };
  }
  if (query.dataSourceType === 'profiles') {
    return { pathname: '/profiles', queryParam: 'q' };
  }

  const streamType = firstString(
    query.query.streamType,
    query.query.stream_type,
  )?.toLowerCase();
  if (streamType === 'traces') {
    return { pathname: '/traces', queryParam: 'sql' };
  }
  return { pathname: '/logs', queryParam: 'sql' };
}

function queryLanguage(query: PanelQuery): string {
  return firstString(query.query.language)?.toLowerCase() ?? '';
}

function appendTimeRange(
  params: URLSearchParams,
  timeRange: DashboardTimeRange,
): void {
  const from = microsToIso(timeRange.from);
  const to = microsToIso(timeRange.to);
  params.set('from', from);
  params.set('to', to);
  params.set('time', `${from}..${to}`);
}

function microsToIso(value: number): string {
  return new Date(Math.floor(value / 1_000)).toISOString();
}

function firstString(...values: unknown[]): string | undefined {
  return values.find(
    (value): value is string => typeof value === 'string' && value.trim().length > 0,
  )?.trim();
}

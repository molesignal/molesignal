export const QUERY_LEGEND_AUTO = '__auto';
export const QUERY_LEGEND_VERBOSE = '__verbose';
export const DEFAULT_QUERY_LEGEND_TEMPLATE = '{{label_name}}';

export type QueryLegendMode = 'auto' | 'verbose' | 'custom';

/**
 * Grafana keeps missing/empty legend formats as the legacy verbose behavior,
 * while new queries explicitly persist the smart `__auto` mode.
 */
export function resolveQueryLegendMode(
  legend: string | undefined,
): QueryLegendMode {
  if (legend === QUERY_LEGEND_AUTO) return 'auto';
  if (!legend || legend === QUERY_LEGEND_VERBOSE) return 'verbose';
  return 'custom';
}

export function queryLegendValueForMode(
  mode: QueryLegendMode,
): string | undefined {
  if (mode === 'auto') return QUERY_LEGEND_AUTO;
  if (mode === 'verbose') return undefined;
  return DEFAULT_QUERY_LEGEND_TEMPLATE;
}

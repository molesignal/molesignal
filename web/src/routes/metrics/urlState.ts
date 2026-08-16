import type { MetricsDrawStyle, MetricsStackMode } from './model';
import {
  DEFAULT_METRICS_QUERY_OPTIONS,
  isValidMetricsStep,
  type MetricsQueryOptions,
} from './queryOptions/model';

export interface MetricsUrlState {
  queryOptions: MetricsQueryOptions;
  drawStyle: MetricsDrawStyle;
  stackMode: MetricsStackMode;
}

/** Parse the complete, refresh-safe Metrics Explore presentation state. */
export function metricsStateFromParams(params: URLSearchParams): MetricsUrlState {
  const step = params.get('step')?.trim() ?? '';
  const draw = params.get('draw');
  const stack = params.get('stack_mode');

  return {
    queryOptions: {
      legend: params.get('legend') ?? DEFAULT_METRICS_QUERY_OPTIONS.legend,
      format: params.get('view') === 'table' ? 'table' : 'time_series',
      step: step && isValidMetricsStep(step) ? step : DEFAULT_METRICS_QUERY_OPTIONS.step,
      type: params.get('type') === 'instant' ? 'instant' : 'range',
      exemplars: params.get('exemplars') !== '0',
    },
    drawStyle: draw === 'bar' || draw === 'points' ? draw : 'line',
    stackMode: stack === 'normal' || stack === 'percent' ? stack : 'none',
  };
}

export function writeExecutedMetricsState(
  current: URLSearchParams,
  state: MetricsUrlState & { promql: string },
): URLSearchParams {
  const next = new URLSearchParams(current);
  next.set('promql', state.promql.trim());
  next.delete('q');

  writeNonDefault(next, 'step', state.queryOptions.step, 'auto');
  writeNonDefault(
    next,
    'legend',
    state.queryOptions.legend ?? '',
    DEFAULT_METRICS_QUERY_OPTIONS.legend ?? '',
  );
  writeNonDefault(next, 'view', state.queryOptions.format === 'table' ? 'table' : 'graph', 'graph');
  writeNonDefault(next, 'type', state.queryOptions.type, 'range');
  writeNonDefault(next, 'exemplars', state.queryOptions.exemplars ? '1' : '0', '1');
  writeNonDefault(next, 'draw', state.drawStyle, 'line');
  writeNonDefault(next, 'stack_mode', state.stackMode, 'none');
  return next;
}

function writeNonDefault(
  params: URLSearchParams,
  key: string,
  value: string,
  defaultValue: string,
): void {
  if (value === defaultValue || !value) params.delete(key);
  else params.set(key, value);
}

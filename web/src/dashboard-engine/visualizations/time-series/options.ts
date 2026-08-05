import type { TimeSeriesChartOptionsInput } from '@/viz/timeseries/types';

import type { DashboardPanel } from '../../schema';

export function buildTimeSeriesOptions(
  panel: DashboardPanel,
  options: Record<string, unknown>,
): TimeSeriesChartOptionsInput {
  return {
    drawStyle: optionEnum(
      options.drawStyle,
      ['line', 'area', 'bar', 'points'],
      'line',
    ),
    interpolation: optionEnum(
      options.lineInterpolation ?? options.interpolation,
      ['linear', 'stepBefore', 'stepAfter'],
      'linear',
    ),
    lineWidth: optionNumber(options.lineWidth, 1.5),
    fillOpacity: optionNumber(options.fillOpacity, 0),
    showPoints: optionEnum(
      options.showPoints,
      ['auto', 'always', 'never'],
      'auto',
    ),
    stackMode: optionEnum(
      options.stackMode,
      ['none', 'normal', 'percent'],
      'none',
    ),
    tooltipMode: optionEnum(
      options.tooltipMode,
      ['single', 'all', 'hidden'],
      'all',
    ),
    legendMode: optionEnum(
      options.legendMode,
      ['list', 'table', 'hidden'],
      'table',
    ),
    legendPlacement: optionEnum(
      options.legendPlacement,
      ['bottom', 'right'],
      'bottom',
    ),
    legendStats: optionEnumArray(
      options.legendStats,
      ['last', 'min', 'max', 'mean', 'sum'],
      ['last', 'min', 'max', 'mean'],
    ),
    thresholds: (panel.fieldConfig.thresholds?.steps ?? [])
      .filter(
        (step): step is typeof step & { value: number } =>
          typeof step.value === 'number',
      )
      .map((step) => ({
        value: step.value,
        color: step.color,
        ...(step.label ? { label: step.label } : {}),
        showLine: true,
      })),
    annotations: Array.isArray(options.annotations)
      ? options.annotations.filter(isTimeSeriesAnnotation)
      : [],
  };
}

function optionNumber(value: unknown, fallback: number): number {
  return typeof value === 'number' && Number.isFinite(value) ? value : fallback;
}

function optionEnum<T extends string>(
  value: unknown,
  options: readonly T[],
  fallback: T,
): T {
  return typeof value === 'string' && options.includes(value as T)
    ? (value as T)
    : fallback;
}

function optionEnumArray<T extends string>(
  value: unknown,
  options: readonly T[],
  fallback: readonly T[],
): T[] {
  if (!Array.isArray(value)) return [...fallback];
  return value.filter(
    (item): item is T =>
      typeof item === 'string' && options.includes(item as T),
  );
}

function isTimeSeriesAnnotation(
  value: unknown,
): value is {
  id: string;
  timestamp: number;
  label: string;
  color?: string;
  endTimestamp?: number;
} {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return false;
  const annotation = value as Record<string, unknown>;
  return (
    typeof annotation.id === 'string' &&
    typeof annotation.timestamp === 'number' &&
    typeof annotation.label === 'string'
  );
}

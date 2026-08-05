import { timeSeriesColor, timeSeriesColors } from '@/viz/timeseries/colors';

export function visualizationColor(key: string): string {
  return timeSeriesColor(key);
}

export function visualizationColors(keys: readonly string[]): string[] {
  return timeSeriesColors(keys);
}

export function heatmapColor(scheme: unknown): string {
  if (scheme === 'greens') return 'var(--green)';
  if (scheme === 'reds') return 'var(--red)';
  if (scheme === 'spectrum') return 'var(--chart-6)';
  return 'var(--blue)';
}

export function stableValueKey(value: unknown): string {
  if (value === null) return 'null';
  if (value === undefined) return 'undefined';
  if (typeof value === 'object') {
    try {
      return JSON.stringify(value);
    } catch {
      return String(value);
    }
  }
  return `${typeof value}:${String(value)}`;
}

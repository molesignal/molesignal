import { http } from '@/lib/http';
import type { RumAlertMetric, RumAlertQuery } from '@/types/alerting';

/** The figure of a rule over one window: no value where nothing happened or too few rows did. */
export interface RumAlertPoint {
  /** Where the window begins, in microseconds. */
  at: number;
  value: number | null;
  /** The rows the figure was made of. */
  samples: number | null;
}

export interface RumAlertPreview {
  metric: RumAlertMetric;
  /** The unit of the figure; a query's is that of its measure. */
  unit: 'count' | 'percent' | 'milliseconds' | 'score' | 'bytes' | 'none';
  /** How many rows the figure needs in a window before it has a value. */
  minSamples: number;
  periodSecs: number;
  from: number;
  to: number;
  points: RumAlertPoint[];
  /** The last window, which ends at `to`. */
  current: RumAlertPoint | null;
  unavailable?: 'time_budget' | 'busy' | 'timestamp_masked';
}

export interface RumAlertPreviewParams {
  query: RumAlertQuery;
  /** The window the rule is evaluated over, which is how long a point is. */
  periodSecs: number;
  /** Microseconds; the last stretch of time ending now when left out. */
  from?: number;
  to?: number;
}

/**
 * What a rule on a RUM figure would have seen over the last stretch of time, worked out by the
 * code that evaluates the rule: the editor's curve and a template's value now are read here.
 */
export async function previewAlert(params: RumAlertPreviewParams, signal?: AbortSignal): Promise<RumAlertPreview> {
  const { data } = await http.post<RumAlertPreview>('/rum/alerts/preview', params, signal ? { signal } : undefined);

  return data;
}

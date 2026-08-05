import { http, toApiError } from '@/lib/http';
import type { Incident, IncidentRca } from '@/types/alerting';

/**
 * List incidents. Phase 6 M1 contract: backend truncates `trace_ids`,
 * `host_ids`, `affected_services` to top 1 element each and returns
 * `triggering_query = null` to keep the list payload small. Use `get`
 * for the full context.
 */
export async function list(options?: {
  scope?: 'active' | 'all';
  window_secs?: number;
}): Promise<Incident[]> {
  const { data } = await http.get<Incident[]>('/alerts/incidents', {
    params: options,
  });
  return data;
}

/**
 * Fetch a single incident with full cross-signal context — all
 * `trace_ids`, full `triggering_query.sample_values`. Backend enforces
 * org isolation on this endpoint (Phase 6 M1 addition).
 */
export async function get(id: string): Promise<Incident> {
  const { data } = await http.get<Incident>(`/alerts/incidents/${encodeURIComponent(id)}`);
  return data;
}

/** Aggregated alert insights over a look-back window . */
export interface AlertInsights {
  window_secs: number;
  total: number;
  active: number;
  closed: number;
  mttr_secs: number;
  noise_rate: number;
  /** 24 buckets: incident count by UTC hour-of-day of creation. */
  by_hour: number[];
  by_severity: Record<string, number>;
  top_services: Array<{ key: string; count: number }>;
  top_rules: Array<{ key: string; count: number }>;
}

export async function insights(windowSecs?: number): Promise<AlertInsights> {
  const suffix = windowSecs ? `?window_secs=${windowSecs}` : '';
  const { data } = await http.get<AlertInsights>(`/alerts/insights${suffix}`);
  return data;
}

/**
 * Fetch the AI root-cause analysis for an incident. Returns `null` when
 * none exists yet (backend replies 404) so callers can show an empty
 * state without treating it as an error; other failures re-throw.
 */
export async function getRca(id: string): Promise<IncidentRca | null> {
  try {
    const { data } = await http.get<IncidentRca>(
      `/alerts/incidents/${encodeURIComponent(id)}/rca`,
    );
    return data;
  } catch (err) {
    if (toApiError(err).status === 404) return null;
    throw err;
  }
}

export type RcaLocale = 'en-us' | 'zh-cn';

/**
 * RCA only supports the product's two UI languages. Keep this normalization
 * client-side as well as server-side so arbitrary locale text never becomes
 * part of the model prompt.
 */
export function normalizeRcaLocale(locale?: string): RcaLocale {
  return locale?.trim().toLowerCase().startsWith('zh') ? 'zh-cn' : 'en-us';
}

/**
 * Trigger on-demand RCA generation (synchronous: blocks while the LLM
 * runs, then returns the stored analysis). Requires the intelligence feature.
 */
export async function generateRca(id: string, locale?: string): Promise<IncidentRca> {
  const { data } = await http.post<IncidentRca>(
    `/alerts/incidents/${encodeURIComponent(id)}/rca`,
    undefined,
    { params: { locale: normalizeRcaLocale(locale) } },
  );
  return data;
}

export async function ack(id: string) {
  await http.post(`/alerts/incidents/${encodeURIComponent(id)}/ack`);
}

export async function resolve(id: string) {
  await http.post(`/alerts/incidents/${encodeURIComponent(id)}/resolve`);
}

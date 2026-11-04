import { http, toApiError } from '@/lib/http';
import type {
  BulkAction,
  BulkResult,
  Incident,
  IncidentRca,
  IncidentStatus,
  IncidentTimeline,
  Priority,
  Reassignment,
  Severity,
  TimelineEntry,
  Urgency,
} from '@/types/alerting';

/**
 * List incidents. The backend truncates `trace_ids`,
 * `host_ids`, `affected_services` to top 1 element each and returns
 * `triggering_query = null` to keep the list payload small. Use `get`
 * for the full context.
 */
export async function list(options?: {
  scope?: 'active' | 'all';
  windowSecs?: number;
}): Promise<Incident[]> {
  const { data } = await http.get<Incident[]>('/alerts/incidents', {
    params: options,
  });
  return data;
}

/**
 * Fetch a single incident with full cross-signal context — all
 * `trace_ids`, full `triggering_query.sample_values`. Backend enforces
 * organization isolation on this endpoint.
 */
export async function get(id: string): Promise<Incident> {
  const { data } = await http.get<Incident>(`/alerts/incidents/${encodeURIComponent(id)}`);
  return data;
}

/** Aggregated alert insights over a look-back window . */
export interface AlertInsights {
  windowSecs: number;
  total: number;
  active: number;
  closed: number;
  mttrSecs: number;
  noiseRate: number;
  /** 24 buckets: incident count by UTC hour-of-day of creation. */
  byHour: number[];
  bySeverity: Record<string, number>;
  topServices: Array<{ key: string; count: number }>;
  topRules: Array<{ key: string; count: number }>;
  /** How people responded: this window, the one before it, and broken down. */
  response: ResponseInsights;
}

/** Mean and nearest-rank percentiles, in seconds. */
export interface Durations {
  meanSecs: number;
  p50Secs: number;
  p90Secs: number;
}

export interface ResponseSummary {
  incidents: number;
  /** Incidents someone took: acknowledged, or resolved by a person. */
  responded: number;
  mtta: Durations;
  mttr: Durations;
  /** 0..1: incidents that reached step 2 or were escalated by hand. */
  escalationRate: number;
  /** Per escalation step (from 0): how many incidents reached it and were acknowledged in it. */
  steps: Array<{ step: number; reached: number; acknowledged: number }>;
  /** How the first response came: web, api, im, sms, phone or agent. */
  ackVia: Record<string, number>;
}

export interface BreakdownRow {
  key: string;
  incidents: number;
  mttaSecs: number;
  mttrSecs: number;
  escalationRate: number;
}

export interface ResponseInsights {
  current: ResponseSummary;
  previous: ResponseSummary;
  breakdown: {
    teams: BreakdownRow[];
    services: BreakdownRow[];
    people: BreakdownRow[];
    /** Incidents without a priority are left out. */
    priorities: BreakdownRow[];
  };
}

/** One person's on-call load over a window. */
export interface PersonLoad {
  userId: string;
  oncallSecs: number;
  pages: number;
  nightPages: number;
  weekendPages: number;
  interruptedNights: number;
  /** The timezone nights and weekends were judged in. */
  timezone: string;
  responded: number;
  mttaSecs: number;
}

export interface OncallLoad {
  windowSecs: number;
  nightStartHour: number;
  nightEndHour: number;
  people: PersonLoad[];
}

/** A past incident like this one, and where they are alike. */
export interface SimilarIncident {
  id: string;
  summary: string;
  severity: Severity;
  status: IncidentStatus;
  createdAt: number;
  resolvedAt: number | null;
  resolvedBy: string | null;
  score: number;
  /** fingerprint, rule, services, labels, title or team. */
  reasons: string[];
  notes: number;
  rootCause: string | null;
}

/** A change reported near an incident's trigger, or its rule edited the day before. */
export interface IncidentChange {
  kind: 'deployment' | 'configuration' | 'feature_flag' | 'rule';
  at: number;
  service?: string;
  /** A change of a service the incident's service calls. */
  downstream: boolean;
  version?: string;
  status?: 'succeeded' | 'failed' | 'rolled_back';
  commit?: string;
  description?: string;
  author?: string;
  link?: string;
  ruleId?: string;
  ruleName?: string;
}

export interface IncidentChanges {
  from: number;
  to: number;
  changes: IncidentChange[];
}

export async function similar(id: string): Promise<SimilarIncident[]> {
  const { data } = await http.get<{ incidents: SimilarIncident[] }>(path(id, 'similar'));
  return data.incidents;
}

export async function changes(id: string): Promise<IncidentChanges> {
  const { data } = await http.get<IncidentChanges>(path(id, 'changes'));
  return data;
}

export async function insights(windowSecs?: number): Promise<AlertInsights> {
  const suffix = windowSecs ? `?windowSecs=${windowSecs}` : '';
  const { data } = await http.get<AlertInsights>(`/alerts/insights${suffix}`);
  return data;
}

/** Everyone's on-call load over the window, or only one schedule's members and shifts. */
export async function oncallLoad(windowSecs: number, scheduleId?: string): Promise<OncallLoad> {
  const params = new URLSearchParams({ windowSecs: String(windowSecs) });
  if (scheduleId) params.set('scheduleId', scheduleId);
  const { data } = await http.get<OncallLoad>(`/alerts/insights/oncall-load?${params.toString()}`);
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
 * runs, then returns the stored analysis). Requires the agent feature.
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

const path = (id: string, action: string) =>
  `/alerts/incidents/${encodeURIComponent(id)}/${action}`;

export async function reassign(id: string, to: Reassignment): Promise<Incident> {
  const { data } = await http.post<Incident>(path(id, 'reassign'), to);
  return data;
}

export async function addResponders(id: string, userIds: string[]): Promise<Incident> {
  const { data } = await http.post<Incident>(path(id, 'responders'), { userIds: userIds });
  return data;
}

/** Escalates to `step` (from 0), or to the next step when none is given. */
export async function escalate(id: string, step?: number): Promise<Incident> {
  const { data } = await http.post<Incident>(path(id, 'escalate'), step === undefined ? {} : { step });
  return data;
}

export async function snooze(id: string, durationSecs: number): Promise<Incident> {
  const { data } = await http.post<Incident>(path(id, 'snooze'), { durationSecs: durationSecs });
  return data;
}

export async function cancelSnooze(id: string): Promise<Incident> {
  const { data } = await http.delete<Incident>(path(id, 'snooze'));
  return data;
}

/** Notify as usual: its held notifications go out and an inhibit rule no longer holds it back. */
export async function liftInhibition(id: string): Promise<Incident> {
  const { data } = await http.post<Incident>(path(id, 'inhibition/lift'));
  return data;
}

export async function setSeverity(id: string, severity: Severity): Promise<Incident> {
  const { data } = await http.put<Incident>(path(id, 'severity'), { severity });
  return data;
}

/** Who a hand-made incident goes to: routing as if it had fired, a team, a policy, or named people. */
export type NewIncidentAssignment =
  | { kind: 'auto' }
  | { kind: 'team'; teamId: string }
  | { kind: 'policy'; escalationPolicyId: string }
  | { kind: 'users'; userIds: string[] };

/** An incident a person opens by hand: what they report, how bad, and who should take it. */
export interface NewIncidentInput {
  title: string;
  /** Markdown, kept as the `description` annotation. */
  description?: string;
  severity: Severity;
  /** Left out, the escalation policy's urgency rule decides. */
  urgency?: Urgency;
  priority?: Priority;
  assign: NewIncidentAssignment;
  /** Catalog services it affects, by name. */
  services?: string[];
  labels?: Record<string, string>;
  /** Declared a major incident as it opens. */
  declareMajor?: boolean;
}

export async function create(input: NewIncidentInput): Promise<Incident> {
  const { data } = await http.post<Incident>('/alerts/incidents', input);
  return data;
}

/** `null` clears it. Nobody is paged again. */
export async function setPriority(id: string, priority: Priority | null): Promise<Incident> {
  const { data } = await http.put<Incident>(path(id, 'priority'), { priority });
  return data;
}

/** Raised to high, an unacknowledged incident is paged again from the first step. */
export async function setUrgency(id: string, urgency: Urgency): Promise<Incident> {
  const { data } = await http.put<Incident>(path(id, 'urgency'), { urgency });
  return data;
}

/** Merges `sourceIds` into the incident `id`. */
export async function merge(id: string, sourceIds: string[]): Promise<Incident> {
  const { data } = await http.post<Incident>(path(id, 'merge'), { sourceIds: sourceIds });
  return data;
}

export async function addNote(id: string, text: string): Promise<TimelineEntry> {
  const { data } = await http.post<TimelineEntry>(path(id, 'notes'), { text });
  return data;
}

export async function timeline(id: string): Promise<IncidentTimeline> {
  const { data } = await http.get<IncidentTimeline>(path(id, 'timeline'));
  return data;
}

export async function bulk(ids: string[], action: BulkAction): Promise<BulkResult[]> {
  const { data } = await http.post<{ results: BulkResult[] }>('/alerts/incidents/bulk', {
    ids,
    ...action,
  });
  return data.results;
}

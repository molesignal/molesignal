import type { LabelMatcher } from '@/api/semanticGroups';

export type { LabelMatcher, MatchOp } from '@/api/semanticGroups';

export type Severity = 'info' | 'warning' | 'error' | 'critical';
export type IncidentStatus = 'open' | 'acknowledged' | 'resolved' | 'closed';
export type ComparisonOp = 'gt' | 'gte' | 'lt' | 'lte' | 'eq' | 'neq';
export type AlertRuleKind = 'scheduled' | 'real_time' | 'anomaly';
export type StreamType = 'logs' | 'metrics' | 'traces';
export type AlertRuleState =
  | { kind: 'healthy' }
  | { kind: 'pending'; consecutive: number }
  | { kind: 'firing' };

/** Anomaly detector params (`AlertRule.kind === 'anomaly'`). Mirrors the
 *  backend `AnomalyParams`; `mad` and `ewma` are implemented today. */
export interface AnomalyParams {
  algorithm: string;
  /** Days of same-time-of-day history used as the baseline (1–30). */
  lookback_days: number;
  /** Sigma multiplier; current value fires when it deviates > k·σ. */
  k: number;
  /** EWMA smoothing factor α∈(0,1]; only used when `algorithm === 'ewma'`. */
  alpha?: number;
  /** Build the baseline only from same-weekday history (cuts weekday/weekend
   *  false positives). Default false; requires lookback_days >= 7. */
  weekly_seasonality?: boolean;
}

/** One graded threshold of a multi-level rule. The backend evaluates every
 *  threshold independently and the highest matured band sets the incident
 *  severity (e.g. disk 85 %→warning, 95 %→critical). Mirrors domain
 *  `SeverityThreshold`. */
export interface SeverityThreshold {
  severity: Severity;
  operator: ComparisonOp;
  threshold: number;
  for_periods: number;
}

export interface AlertRule {
  id: string;
  org_id: string;
  name: string;
  description: string;
  enabled: boolean;
  /** Evaluation pipeline; omitted payloads default to `scheduled` on the backend. */
  kind?: AlertRuleKind;
  query: {
    language: 'sql' | 'promql';
    statement: string;
    period_secs: number;
    /** Target stream/table — required by the backend evaluator to run the query. */
    stream?: { name: string; stream_type: StreamType };
  };
  trigger: {
    operator: ComparisonOp;
    threshold: number;
    for_periods: number;
    silence_secs: number;
  };
  /** Graded thresholds; empty/omitted falls back to the single `trigger`. The
   *  evaluator scores each band and the highest matched one wins. */
  thresholds?: SeverityThreshold[];
  /** Explicit fallback severity for a single-band rule; `null` lets the
   *  evaluator derive it. */
  severity?: Severity | null;
  /** Present only when `kind === 'anomaly'`. */
  anomaly_params?: AnomalyParams | null;
  escalation_policy_id: string;
  labels: Record<string, string>;
  annotations: Record<string, string>;
  /** Epoch microseconds of the last completed evaluator pass. */
  last_eval_at?: number | null;
  last_state?: AlertRuleState;
  created_at?: number;
  updated_at?: number;
}

export interface TriggeringSample {
  /** Microseconds since epoch — same axis as `Incident.created_at`. */
  ts: number;
  value: number;
  labels: Record<string, string>;
}

export interface TriggeringQuery {
  language: 'sql' | 'promql';
  statement: string;
  /** Up to 20 sampled rows from the result set that caused the incident
   *  to fire. Used by the detail drawer to show "this is what the
   *  backend saw". */
  sample_values: TriggeringSample[];
}

export interface Incident {
  id: string;
  org_id: string;
  rule_id: string;
  escalation_policy_id: string;
  status: IncidentStatus;
  severity: Severity;
  summary: string;
  fingerprint: string;
  current_step: number;
  current_loop: number;
  current_step_started_at: number;
  assignees: string[];
  created_at: number;
  acknowledged_at?: number;
  acknowledged_by?: string;
  resolved_at?: number;
  resolved_by?: string;
  /**
   * Cross-signal context. Phase 6 M1 backend addition (selected
   * "Option C" — see BACKEND_REQUIREMENTS.md).
   *
   * The `list` endpoint truncates `trace_ids` / `host_ids` /
   * `affected_services` to the top 1 element and zeroes
   * `triggering_query`; the detail endpoint returns the complete sets.
   * All fields are `serde(default)` on the backend, so older payloads
   * deserialize with empty defaults.
   */
  labels: Record<string, string>;
  annotations: Record<string, string>;
  trace_ids: string[];
  host_ids: string[];
  affected_services: string[];
  triggering_query: TriggeringQuery | null;
}

/**
 * AI root-cause analysis for an incident. Produced asynchronously by a
 * background sweeper (active incidents) or on demand via `POST
 * /alerts/incidents/{id}/rca`. `GET` returns 404 until one exists.
 */
export interface IncidentRca {
  incident_id: string;
  org_id: string;
  summary: string;
  provider: string | null;
  model: string | null;
  prompt_builtin_key: string | null;
  prompt_hash: string | null;
  prompt_tokens: number;
  completion_tokens: number;
  finish_reason: string | null;
  created_at: number;
  updated_at: number;
}

export type RotationKind =
  | 'daily'
  | 'weekly'
  | { custom: { period_secs: number } };

/** Restricts a rotation to certain weekdays/hours. `weekday_mask` is a
 *  bitfield: bit 0 = Sunday … bit 6 = Saturday. */
export interface ActiveWindow {
  weekday_mask: number;
  hour_start: number;
  hour_end: number;
}

export interface Rotation {
  id: string;
  name: string;
  members: string[];
  kind: RotationKind;
  /** `null`/omitted means the rotation is always active. The backend requires
   *  the field to be present on write, so send `null` rather than omitting. */
  active_window?: ActiveWindow | null;
  start_at: number;
}

export interface ScheduleOverride {
  id: string;
  user_id: string;
  start_at: number;
  end_at: number;
  reason: string;
}

export interface Schedule {
  id: string;
  org_id: string;
  name: string;
  description: string;
  team_id?: string | null;
  timezone: string;
  enabled: boolean;
  rotations: Rotation[];
  overrides: ScheduleOverride[];
  created_by?: string | null;
  updated_by?: string | null;
  created_at: number;
  updated_at: number;
}

export type EscalationTarget =
  | { kind: 'user'; user_id: string }
  | { kind: 'schedule'; schedule_id: string }
  | { kind: 'team'; team_id: string };

export interface EscalationStep {
  targets: EscalationTarget[];
  ack_timeout_secs: number;
  /** Level routing: the step only fires when `incident.severity >=
   *  min_severity`. `null`/omitted means it applies to every severity. */
  min_severity?: Severity | null;
}

export interface EscalationPolicy {
  id: string;
  org_id: string;
  name: string;
  steps: EscalationStep[];
  repeat: boolean;
  max_loops: number;
}

/* ───────────────────────── mute ───────────────────────── */

/** Window during which a mute rule suppresses delivery. `start`/`end` are
 *  micros-epoch; `weekday_mask` bit0=Sun … bit6=Sat. Mirrors domain
 *  `MuteWindow` (serde `tag = "type"`). */
export type MuteWindow =
  | { type: 'fixed'; start: number; end: number }
  | {
      type: 'recurring';
      timezone: string;
      weekday_mask: number;
      hour_start: number;
      hour_end: number;
    };

/** Alert mute (silence): when every matcher hits and the window is active the
 *  dispatcher pauses delivery — the incident is still recorded. */
export interface MuteRule {
  id: string;
  org_id: string;
  name: string;
  enabled: boolean;
  matchers: LabelMatcher[];
  window: MuteWindow;
  comment: string;
  created_by?: string | null;
  created_at: number;
  updated_at: number;
}

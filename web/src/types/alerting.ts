import type { FindingKind } from '@/api/discovery';
import type { LabelMatcher } from '@/api/semanticGroups';

export type { LabelMatcher, MatchOp } from '@/api/semanticGroups';

export type Severity = 'info' | 'warning' | 'error' | 'critical';
/** Business weight, P1 the heaviest; an incident may have none. Never derived from severity. The
 * organization chooses how many levels (up to nine) and their names; the keys stay P1, P2, …. */
export type Priority = `P${1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9}`;
/** Whether to wake someone now: the key of an urgency level of the organization. `high` and `low`
 * are built in; an organization can add more (see the importance settings). */
export type Urgency = string;
export type IncidentStatus = 'open' | 'acknowledged' | 'resolved' | 'closed';
export type ComparisonOp = 'gt' | 'gte' | 'lt' | 'lte' | 'eq' | 'neq';
export type AlertRuleKind =
  | 'scheduled'
  | 'real_time'
  | 'anomaly'
  | 'composite'
  | 'forecast'
  | 'outlier'
  | 'heartbeat';

/** A composite rule: other rules, named A to Z, combined with &&, || and !; a rule is true while it has an open incident. */
export interface CompositeQuery {
  expression: string;
  /** Letter → rule id. */
  rules: Record<string, string>;
  /** Merge the open incidents of the combined rules under the composite one's. */
  mergeChildren?: boolean;
}

export type ForecastModel = 'linear' | 'daily' | 'weekly';

/** A forecast rule: the series of the last `lookback_days`, carried forward `lead_secs`. */
export interface ForecastParams {
  lookbackDays: number;
  leadSecs: number;
  model: ForecastModel;
}

export type OutlierDirection = 'both' | 'above' | 'below';

/** An outlier rule: the groups of a split query compared with each other, `tolerance` scaled deviations apart. */
export interface OutlierParams {
  tolerance: number;
  direction: OutlierDirection;
}

/** A heartbeat rule: a check-in expected every `period_secs`, `grace_secs` late at most. */
export interface HeartbeatParams {
  periodSecs: number;
  graceSecs: number;
}
export type StreamType = 'logs' | 'metrics' | 'traces';
export type AlertRuleState =
  | { kind: 'healthy' }
  | { kind: 'pending'; consecutive: number }
  | { kind: 'firing' }
  /** The last evaluation had no value and the rule kept its state (`no_data` is `keep` or `alert`). */
  | { kind: 'no_data' }
  /** The rule fired, but an alert pipeline dropped the event before its incident opened. */
  | { kind: 'dropped' };

/** What a round without a value counts as. */
export type NoDataAction = 'resolve' | 'keep' | 'alert' | 'zero';

export interface NoDataCondition {
  action: NoDataAction;
  /** Rounds without a value in a row before a no-data incident opens (`alert`); 0 is one. */
  forPeriods?: number;
}

export interface RecoveryCondition {
  /** The line an open incident waits for the value to cross back over; the threshold when absent. */
  threshold?: number | null;
  /** Rounds on the healthy side in a row before it recovers; 0 is one. */
  forPeriods?: number;
}

/** Anomaly detector params (`AlertRule.kind === 'anomaly'`). Mirrors the
 *  backend `AnomalyParams`; `mad` and `ewma` are implemented today. */
export interface AnomalyParams {
  algorithm: string;
  /** Days of same-time-of-day history used as the baseline (1–30). */
  lookbackDays: number;
  /** Sigma multiplier; current value fires when it deviates > k·σ. */
  k: number;
  /** EWMA smoothing factor α∈(0,1]; only used when `algorithm === 'ewma'`. */
  alpha?: number;
  /** Build the baseline only from same-weekday history (cuts weekday/weekend
   *  false positives). Default false; requires lookback_days >= 7. */
  weeklySeasonality?: boolean;
}

/** One graded threshold of a multi-level rule. The backend evaluates every
 *  threshold independently and the highest matured band sets the incident
 *  severity (e.g. disk 85 %→warning, 95 %→critical). Mirrors domain
 *  `SeverityThreshold`. */
export interface SeverityThreshold {
  severity: Severity;
  operator: ComparisonOp;
  threshold: number;
  forPeriods: number;
}

/** A figure of APM an alert rule watches, in the unit thresholds are set in. */
export type ApmAlertMetric =
  | 'error_rate'
  | 'p50'
  | 'p90'
  | 'p95'
  | 'p99'
  | 'average'
  | 'throughput'
  | 'request_count'
  | 'error_count'
  | 'new_error_groups'
  | 'apdex'
  | 'slo_burn_rate';

export interface ApmAlertQuery {
  namespace?: string | null;
  service: string;
  environment?: string | null;
  /** One endpoint of the service; the whole service when absent. */
  transaction?: { name: string; kind: 'http' | 'rpc' | 'messaging' | 'span' | 'other' } | null;
  metric: ApmAlertMetric;
  sloId?: string | null;
  /** A shorter window a burn rate must pass the threshold over as well. */
  shortWindowSecs?: number | null;
}

/** A figure of RUM an alert rule watches, in the unit thresholds are set in. */
export type RumAlertMetric =
  | 'session_count'
  | 'view_count'
  | 'error_count'
  | 'error_rate'
  | 'crash_free_sessions'
  | 'frustration_rate'
  | 'failed_request_rate'
  | 'lcp_p75'
  | 'inp_p75'
  | 'cls_p75'
  | 'ttfb_p75'
  | 'new_issues'
  | 'regressed_issues'
  | 'issue_impact'
  | 'custom_timing'
  | 'custom_vital'
  | 'query';

/** What a rule takes of a figure the application measures itself. */
export type RumAlertStatistic = 'p50' | 'p75' | 'p90' | 'p95' | 'p99' | 'avg' | 'count';

/**
 * The query of a rule made from the RUM explorer: one number over the rows of one type that the
 * query matches. The query is kept as written and as the filters and words it was read into,
 * which are what the server evaluates.
 */
export interface RumAlertSearch {
  /** The type of rows, as the explorer names it: `views`, `errors`. */
  type: string;
  statement?: string;
  filters?: Array<{ field: string; op: string; value: string }>;
  freeText?: string[];
  measure: { fn: string; field?: string };
}

export interface RumAlertQuery {
  applicationId: string;
  /** Every environment of the application when absent. */
  environment?: string | null;
  /** One page (or, for an app, one screen); every page when absent. */
  page?: string | null;
  /** One version of the application; every version when absent. */
  version?: string | null;
  metric: RumAlertMetric;
  /** How many of the rows the figure is made of it needs before it has a value; the metric's default when absent. */
  minSamples?: number | null;
  /** The name of the timing or the vital a rule on a figure the application measures watches. */
  customName?: string | null;
  /** What it takes of that figure; the 75th percentile when absent. */
  statistic?: RumAlertStatistic | null;
  /** The explorer query a rule on `query` watches. */
  search?: RumAlertSearch | null;
}

/** Which findings of automatic discovery a rule takes; an empty list takes every one. */
export interface DiscoveryQuery {
  kinds?: FindingKind[];
  /** APM services (`namespace/name` or the name alone) or RUM application ids. */
  subjects?: string[];
  environments?: string[];
  minSeverity?: Severity | null;
}

export interface AlertRule {
  id: string;
  orgId: string;
  name: string;
  description: string;
  enabled: boolean;
  /** Evaluation pipeline; omitted payloads default to `scheduled` on the backend. */
  kind?: AlertRuleKind;
  query: {
    language: 'sql' | 'promql';
    statement: string;
    periodSecs: number;
    /** Target stream/table — required by the backend evaluator to run the query. */
    stream?: { name: string; streamType: StreamType } | null;
    /** An APM figure watched in place of the statement; such a rule names no stream. */
    apm?: ApmAlertQuery | null;
    /** A RUM figure watched in place of the statement; such a rule names no stream either. */
    rum?: RumAlertQuery | null;
    /** Columns of the result the rule is split by: each group of values opens its own incident. */
    groupBy?: string[];
    composite?: CompositeQuery | null;
    forecast?: ForecastParams | null;
    outlier?: OutlierParams | null;
    heartbeat?: HeartbeatParams | null;
    /** Seconds each round's window is moved back (0–3600); absent leaves it to the deployment. */
    delaySecs?: number | null;
    /** Findings of automatic discovery in place of a query: each matching one opens an incident. */
    discovery?: DiscoveryQuery | null;
    /** The cluster of a federation that decides a real-time rule, adding the other clusters' counts; absent lets every cluster decide by itself. */
    homeCluster?: string | null;
  };
  trigger: {
    operator: ComparisonOp;
    threshold: number;
    forPeriods: number;
    silenceSecs: number;
    /** What a round without a value counts as; a threshold rule resolves and an anomaly rule keeps its state when absent. */
    noData?: NoDataCondition | null;
    /** When an open incident recovers; one round past the threshold when absent. */
    recovery?: RecoveryCondition | null;
    /** The priority its incidents open with; none when absent (an alert pipeline may still set one). */
    priority?: Priority | null;
    /** The urgency its incidents open with; the escalation policy's urgency rule decides when absent. */
    urgency?: Urgency | null;
  };
  /** Graded thresholds; empty/omitted falls back to the single `trigger`. The
   *  evaluator scores each band and the highest matched one wins. */
  thresholds?: SeverityThreshold[];
  /** Explicit fallback severity for a single-band rule; `null` lets the
   *  evaluator derive it. */
  severity?: Severity | null;
  /** Present only when `kind === 'anomaly'`. */
  anomalyParams?: AnomalyParams | null;
  escalationPolicyId: string;
  labels: Record<string, string>;
  annotations: Record<string, string>;
  /** Epoch microseconds of the last completed evaluator pass. */
  lastEvalAt?: number | null;
  lastState?: AlertRuleState;
  createdAt?: number;
  updatedAt?: number;
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
  sampleValues: TriggeringSample[];
}

export interface Incident {
  id: string;
  orgId: string;
  ruleId: string;
  escalationPolicyId: string;
  status: IncidentStatus;
  severity: Severity;
  summary: string;
  fingerprint: string;
  currentStep: number;
  currentLoop: number;
  currentStepStartedAt: number;
  assignees: string[];
  createdAt: number;
  acknowledgedAt?: number;
  acknowledgedBy?: string;
  resolvedAt?: number;
  resolvedBy?: string;
  /**
   * Cross-signal incident context. The `list` endpoint truncates `trace_ids` / `host_ids` /
   * `affected_services` to the top 1 element and zeroes
   * `triggering_query`; the detail endpoint returns the complete sets.
   * All fields are `serde(default)` on the backend, so older payloads
   * deserialize with empty defaults.
   */
  labels: Record<string, string>;
  annotations: Record<string, string>;
  traceIds: string[];
  hostIds: string[];
  affectedServices: string[];
  triggeringQuery: TriggeringQuery | null;
  /** The team routing gave the incident when it opened. */
  teamId?: string | null;
  /** How it was routed; incidents opened before routing have none. */
  routing?: IncidentRouting | null;
  /** Snoozed until then (micros); it comes back unacknowledged at that time. */
  snoozedUntil?: number | null;
  snoozedBy?: string | null;
  /** The assignees were named by hand; they hold until the next escalation step. */
  assigneesPinned?: boolean;
  /** Who set the severity by hand; the rule no longer changes it. */
  severitySetBy?: string | null;
  /** The incident it was merged into. */
  mergedInto?: string | null;
  /** Held back by an inhibit rule, because of another incident: its notifications wait. */
  inhibition?: Inhibition | null;
  /** It could be inhibited: its notifications wait until then, to see whether the worse one comes. */
  inhibitionWaitUntil?: number | null;
  /** Who said to notify as usual; it is not inhibited again. */
  inhibitionLiftedBy?: string | null;
  dispatchRound?: number;
  dispatchReason?: EscalationReason;
  /** Absent on incidents opened before urgency existed: they count as high. */
  urgency?: Urgency;
  /** Who set the urgency by hand; support hours no longer change it. */
  urgencySetBy?: string | null;
  /** The urgency came from the policy's support hours, so their start may raise it. */
  urgencyByPolicy?: boolean;
  priority?: Priority | null;
  revision?: number;
}

/** Why an escalation notification went out. */
export type EscalationReason =
  | 'step'
  | 'manual'
  | 'reassigned'
  | 'responder_added'
  | 'snooze_ended'
  | 'ack_timeout'
  | 'severity_raised'
  | 'urgency_raised';

export type TimelineKind =
  | 'acknowledged'
  | 'resolved'
  | 'escalated'
  | 'reassigned'
  | 'responders_added'
  | 'snoozed'
  | 'snooze_cancelled'
  | 'retriggered'
  | 'severity_changed'
  | 'merged'
  | 'merged_in'
  | 'merged_recovered'
  | 'note'
  | 'remediation_requested'
  | 'remediation_executed'
  | 'remediation_verified'
  | 'ticket_created'
  | 'ticket_updated'
  | 'declared_major'
  | 'role_assigned'
  | 'war_room_created'
  | 'task_added'
  | 'task_done'
  | 'status_update'
  | 'step_unreachable'
  | 'pipeline_processed'
  | 'priority_changed'
  | 'urgency_changed'
  | 'inhibited'
  | 'inhibition_ended';

export type ActionVia = 'web' | 'api' | 'im' | 'sms' | 'phone' | 'agent';

/** Why an incident's notifications are held back. The names are kept: the rule may go, the other incident end. */
export interface Inhibition {
  ruleId: string;
  ruleName: string;
  sourceIncidentId: string;
  sourceSummary: string;
  since: number;
  /** The other incident is over; held until then all the same. */
  endingAt?: number | null;
}

/** While an incident of the source side is active, incidents of the target side notify nobody. */
export interface InhibitRule {
  id: string;
  orgId: string;
  name: string;
  enabled: boolean;
  source: { matchers: LabelMatcher[]; minSeverity?: Severity | null };
  target: { matchers: LabelMatcher[]; maxSeverity?: Severity | null };
  /** These labels must be there on both sides, with the same value. */
  equal: string[];
  /** The source must be strictly worse than the target. */
  moreSevereOnly: boolean;
  /** How long a target's notifications wait to see whether a source comes. */
  waitSecs: number;
  /** How long a source that recovered keeps holding targets back. */
  lingerSecs: number;
  comment: string;
  createdBy?: string | null;
  createdAt: number;
  updatedAt: number;
  /** How many incidents it holds back now. */
  inhibiting: number;
}

/** One entry of an incident's timeline. */
export interface TimelineEntry {
  id: string;
  orgId: string;
  incidentId: string;
  at: number;
  kind: TimelineKind;
  /** Who did it; the system did it when empty. */
  actor: string | null;
  detail: Record<string, unknown> & { via?: ActionVia };
}

/** A notification of the incident: to whom, through which connector, and how it went. */
export interface TimelineDelivery {
  id: string;
  eventId: string;
  userId: string | null;
  connectorId: string | null;
  target: string | null;
  stage: string;
  status: string;
  error: string | null;
  createdAt: number;
  deliveredAt: number | null;
}

export interface MergedIncident {
  id: string;
  summary: string;
  status: IncidentStatus;
  severity: Severity;
  createdAt: number;
  resolvedAt: number | null;
}

export interface IncidentTimeline {
  entries: TimelineEntry[];
  deliveries: TimelineDelivery[];
  merged: MergedIncident[];
}

/** Whom an incident is reassigned to. */
export type Reassignment =
  | { kind: 'users'; userIds: string[] }
  | { kind: 'team'; teamId: string }
  | { kind: 'policy'; escalationPolicyId: string };

/** What a bulk request does to every incident it names. */
export type BulkAction =
  | { action: 'acknowledge' }
  | { action: 'resolve' }
  | { action: 'snooze'; durationSecs: number }
  | { action: 'reassign'; to: Reassignment }
  | { action: 'set_severity'; severity: Severity }
  | { action: 'set_priority'; priority: Priority | null }
  | { action: 'set_urgency'; urgency: Urgency };

export interface BulkResult {
  id: string;
  error?: string;
}

/** Why an incident went where it went. */
export type RoutingReason = 'rule' | 'service_owner' | 'team_label' | 'default' | 'source' | 'manual';

export interface IncidentRouting {
  reason: RoutingReason;
  ruleId?: string;
  ruleName?: string;
  /** `namespace/name` of the service whose owning team it went to. */
  service?: string;
}

/**
 * AI root-cause analysis for an incident. Produced asynchronously by a
 * background sweeper (active incidents) or on demand via `POST
 * /alerts/incidents/{id}/rca`. `GET` returns 404 until one exists.
 */
export interface IncidentRca {
  incidentId: string;
  orgId: string;
  summary: string;
  provider: string | null;
  model: string | null;
  promptBuiltinKey: string | null;
  promptHash: string | null;
  promptTokens: number;
  completionTokens: number;
  finishReason: string | null;
  createdAt: number;
  updatedAt: number;
}

export type RotationKind =
  | 'daily'
  | 'weekly'
  | { custom: { periodSecs: number } };

/** A weekly range in local time: the days (`weekday_mask` bit 0 = Sunday …
 *  bit 6 = Saturday) and `HH:MM` start and end. An end not after the start runs
 *  past midnight and belongs to the day it starts; `24:00` only ends a range. */
export interface WeeklyRange {
  weekdayMask: number;
  start: string;
  end: string;
}

/** Weekly ranges read in one IANA timezone. */
/** How some weekly hours read a holiday calendar: holidays left out (make-up workdays count as
 * Mondays), or only holidays counted. */
export interface CalendarUse {
  calendarId: string;
  holidays: 'exclude' | 'only';
}

export interface WeeklyWindows {
  timezone: string;
  ranges: WeeklyRange[];
  calendar?: CalendarUse | null;
}

/** Inside or outside some weekly hours: when a routing rule or an alert pipeline applies. */
export type TimeCondition = { mode: 'inside' | 'outside' } & WeeklyWindows;

/** How an escalation policy sets the urgency of an incident nothing else set it for. */
export type UrgencyRule =
  | { mode: 'by_severity' }
  | { mode: 'always'; urgency: Urgency }
  | {
      mode: 'support_hours';
      hours: WeeklyWindows;
      /** When support hours begin, incidents nobody acknowledged that are less urgent than `inside`
       * are raised to it and page again. */
      raiseAtStart?: boolean;
      /** The urgency inside the hours (high unless set) and outside them (low unless set). */
      inside?: Urgency;
      outside?: Urgency;
    };

export interface Rotation {
  id: string;
  name: string;
  members: string[];
  kind: RotationKind;
  /** Local-time ranges the rotation covers; empty means always. */
  activeWindows: WeeklyRange[];
  /** The holiday calendar the active ranges count by (a whole day when there are none). */
  calendar?: CalendarUse | null;
  startAt: number;
  /** When the rotation stops; without it, it goes on. */
  endAt?: number | null;
}

export interface ScheduleOverride {
  id: string;
  userId: string;
  startAt: number;
  endAt: number;
  reason: string;
}

export interface Schedule {
  id: string;
  orgId: string;
  name: string;
  description: string;
  teamId?: string | null;
  timezone: string;
  enabled: boolean;
  rotations: Rotation[];
  overrides: ScheduleOverride[];
  /** A swap the person asked accepts still waits for someone who manages the schedule. */
  swapNeedsApproval?: boolean;
  createdBy?: string | null;
  updatedBy?: string | null;
  createdAt: number;
  updatedAt: number;
  /** Who is on call when, worked out by the server in the schedule's timezone:
   *  a day back to 14 days ahead on reads. */
  coverage?: ScheduleCoverage | undefined;
}

export type CoverageSource = 'rotation' | 'override' | 'gap';

export interface CoverageSegment {
  startAt: number;
  endAt: number;
  source: CoverageSource;
  userId?: string;
  rotationId?: string;
  overrideId?: string;
  /** Under an override: whom the rotation would have had on call. */
  replacedUserId?: string;
}

export interface ScheduleCoverage {
  from: number;
  to: number;
  segments: CoverageSegment[];
}

export type EscalationTarget =
  | { kind: 'user'; userId: string }
  | { kind: 'schedule'; scheduleId: string }
  | { kind: 'team'; teamId: string };

export interface EscalationStep {
  targets: EscalationTarget[];
  ackTimeoutSecs: number;
  /** Level routing: the step only fires when `incident.severity >=
   *  min_severity`. `null`/omitted means it applies to every severity. */
  minSeverity?: Severity | null;
}

export interface EscalationPolicy {
  id: string;
  orgId: string;
  name: string;
  steps: EscalationStep[];
  repeat: boolean;
  maxLoops: number;
  /** Acknowledged but unresolved this long (seconds), the incident comes back unacknowledged. */
  ackRetriggerSecs?: number | null;
  /** By severity when absent. */
  urgencyRule?: UrgencyRule;
  /** Low-urgency incidents escalate like high ones instead of stopping at the first step. */
  escalateLowUrgency?: boolean;
}

/* ───────────────────────── mute ───────────────────────── */

/** The day of the month a monthly window falls on: a date (a month without it
 *  takes its last day), or the nth weekday (`ordinal` 1–5, -1 = the last;
 *  `weekday` 0 = Sunday). */
export type MonthDay =
  | { kind: 'date'; day: number }
  | { kind: 'weekday'; ordinal: number; weekday: number };

/** Window during which a mute rule suppresses delivery. `start`/`end` of a
 *  fixed window are micros-epoch. Mirrors domain `MuteWindow` (serde
 *  `tag = "type"`). */
export type MuteWindow =
  | { type: 'fixed'; start: number; end: number }
  | { type: 'weekly'; timezone: string; ranges: WeeklyRange[]; calendar?: CalendarUse | null }
  | {
      type: 'monthly';
      timezone: string;
      day: MonthDay;
      start: string;
      durationMinutes: number;
    };

/** Alert mute (silence): when every matcher hits and the window is active the
 *  dispatcher pauses delivery — the incident is still recorded. */
export interface MuteRule {
  id: string;
  orgId: string;
  name: string;
  enabled: boolean;
  matchers: LabelMatcher[];
  window: MuteWindow;
  comment: string;
  createdBy?: string | null;
  createdAt: number;
  updatedAt: number;
  /** Whether the window covers now (the server's answer; `enabled` aside). */
  active?: boolean;
  /** When the window next opens or closes; `null` when it never does again. */
  nextChangeAt?: number | null;
}

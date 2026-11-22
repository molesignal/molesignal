import type { OverviewResponse, ServiceHealth, ServiceSummary } from '@/api/apm';
import type { HomeHealthStatus } from '@/api/home';
import type { MonitorState, SyntheticMonitor } from '@/api/synthetics';
import type { StatusTone } from '@/shell/StatusWord';
import type { AlertRule, Incident, Severity } from '@/types/alerting';

export type HomeWindow = '1h' | '24h' | '7d';

export const HOME_WINDOWS: readonly HomeWindow[] = ['1h', '24h', '7d'];

export const WINDOW_SECS: Record<HomeWindow, number> = {
  '1h': 60 * 60,
  '24h': 24 * 60 * 60,
  '7d': 7 * 24 * 60 * 60,
};

/** Buckets of the intake trend: one per five minutes over an hour, per hour over a day, per six hours over a week. */
export const WINDOW_BUCKETS: Record<HomeWindow, number> = { '1h': 12, '24h': 24, '7d': 28 };

export const SEVERITY_ORDER: readonly Severity[] = ['critical', 'error', 'warning', 'info'];

export const SEVERITY_TONE: Record<Severity, StatusTone> = {
  critical: 'red',
  error: 'orange',
  warning: 'yellow',
  info: 'blue',
};

/** The colour a severity paints an area (a bar, a row's edge) in. */
export const SEVERITY_FILL: Record<Severity, string> = {
  critical: 'var(--red)',
  error: 'var(--orange)',
  warning: 'var(--yellow)',
  info: 'var(--blue)',
};

export const SIGNAL_TONE: Record<HomeHealthStatus, StatusTone> = {
  healthy: 'green',
  delayed: 'yellow',
  degraded: 'red',
  no_data: 'dim',
  unknown: 'dim',
};

export const SERVICE_TONE: Record<ServiceHealth, StatusTone> = {
  critical: 'red',
  warning: 'yellow',
  healthy: 'green',
  no_traffic: 'dim',
};

export const CHECK_TONE: Record<MonitorState, StatusTone> = {
  failing: 'red',
  degraded: 'yellow',
  flaky: 'orange',
  healthy: 'green',
  unknown: 'dim',
};

export interface IncidentSummary {
  /** Open and acknowledged incidents, the most severe first and, within a severity, the oldest first. */
  active: Incident[];
  bySeverity: Record<Severity, number>;
  unacknowledged: number;
}

export function summarizeIncidents(incidents: readonly Incident[]): IncidentSummary {
  const active = incidents
    .filter((incident) => incident.status === 'open' || incident.status === 'acknowledged')
    .sort(
      (a, b) =>
        SEVERITY_ORDER.indexOf(a.severity) - SEVERITY_ORDER.indexOf(b.severity) || a.createdAt - b.createdAt,
    );
  const bySeverity: Record<Severity, number> = { critical: 0, error: 0, warning: 0, info: 0 };

  for (const incident of active) bySeverity[incident.severity] += 1;

  return {
    active,
    bySeverity,
    unacknowledged: active.filter((incident) => incident.status === 'open').length,
  };
}

export interface RuleSummary {
  enabled: number;
  firing: number;
  pending: number;
}

export function summarizeRules(rules: readonly AlertRule[]): RuleSummary {
  const enabled = rules.filter((rule) => rule.enabled);

  return {
    enabled: enabled.length,
    firing: enabled.filter((rule) => rule.lastState?.kind === 'firing').length,
    pending: enabled.filter((rule) => rule.lastState?.kind === 'pending').length,
  };
}

const CHECK_ATTENTION: readonly MonitorState[] = ['failing', 'degraded', 'flaky'];

export interface CheckSummary {
  /** Checks that run: neither drafts, paused nor archived. */
  active: number;
  byState: Record<MonitorState, number>;
  /** Checks not passing, the worst first. */
  attention: SyntheticMonitor[];
}

export function summarizeChecks(monitors: readonly SyntheticMonitor[]): CheckSummary {
  const active = monitors.filter((monitor) => monitor.lifecycle === 'active');
  const byState: Record<MonitorState, number> = { healthy: 0, flaky: 0, degraded: 0, failing: 0, unknown: 0 };

  for (const monitor of active) byState[monitor.state] += 1;

  return {
    active: active.length,
    byState,
    attention: active
      .filter((monitor) => CHECK_ATTENTION.includes(monitor.state))
      .sort((a, b) => CHECK_ATTENTION.indexOf(a.state) - CHECK_ATTENTION.indexOf(b.state) || a.name.localeCompare(b.name)),
  };
}

const HEALTH_RANK: Record<ServiceHealth, number> = { critical: 0, warning: 1, healthy: 2, no_traffic: 3 };

/** The services to look at first: the unhealthy ones, then the busiest. */
export function rankServices(services: readonly ServiceSummary[], limit: number): ServiceSummary[] {
  return [...services]
    .sort(
      (a, b) =>
        HEALTH_RANK[a.health] - HEALTH_RANK[b.health] ||
        b.red.requestCount - a.red.requestCount ||
        a.service.name.localeCompare(b.service.name),
    )
    .slice(0, limit);
}

export interface ApmTrend {
  errorRate: Array<number | null>;
  p95: Array<number | null>;
  requests: number[];
}

/** The figures of every service together, bucket by bucket; a bucket without requests has no rate. */
export function apmTrend(overview: OverviewResponse | undefined): ApmTrend {
  const trend = Array.isArray(overview?.trend) ? overview.trend : [];

  return {
    errorRate: trend.map((point) => ((point.red?.requestCount ?? 0) > 0 ? point.red.errorRate : null)),
    p95: trend.map((point) => ((point.red?.requestCount ?? 0) > 0 ? (point.red.p95Time ?? null) : null)),
    requests: trend.map((point) => point.red?.requestCount ?? 0),
  };
}

export interface Verdict {
  issues: number;
  tone: 'green' | 'yellow' | 'red';
}

/**
 * How the system stands, in one word: the incidents going on, the checks not passing, the
 * services past their lines and the signals not arriving, each counted once.
 */
export function verdictOf(input: {
  incidents?: IncidentSummary | undefined;
  checks?: CheckSummary | undefined;
  serviceHealth?: OverviewResponse['serviceHealth'] | undefined;
  signals?: ReadonlyArray<{ status: HomeHealthStatus }> | undefined;
}): Verdict {
  const { incidents, checks, serviceHealth, signals } = input;
  const severe =
    (incidents ? incidents.bySeverity.critical + incidents.bySeverity.error : 0) +
    (checks?.byState.failing ?? 0) +
    (serviceHealth?.critical ?? 0) +
    (signals ?? []).filter((signal) => signal.status === 'degraded').length;
  const mild =
    (incidents ? incidents.bySeverity.warning + incidents.bySeverity.info : 0) +
    (checks ? checks.byState.degraded + checks.byState.flaky : 0) +
    (serviceHealth?.warning ?? 0) +
    (signals ?? []).filter((signal) => signal.status === 'delayed').length;

  return {
    issues: severe + mild,
    tone: severe > 0 ? 'red' : mild > 0 ? 'yellow' : 'green',
  };
}

/** A share as a percent: two decimals under 1%, one under 10%, none above, so small rates keep their size. */
export function formatShare(value: number | null | undefined): string {
  if (value == null || !Number.isFinite(value)) return '—';
  const percent = value * 100;
  const digits = percent === 0 || percent >= 10 ? 1 : percent < 1 ? 2 : 1;

  return `${percent.toFixed(percent >= 99.95 && percent < 100 ? 2 : digits)}%`;
}

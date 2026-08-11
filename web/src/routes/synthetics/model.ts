import type {
  CreateMonitorInput,
  MonitorDetail,
  MonitorRevision,
  MonitorSchedule,
  SyntheticResult,
} from '@/api/synthetics';

export { monitorTarget, valueSourceText } from '@/api/synthetics';

export function activeRevision(detail: MonitorDetail | undefined): MonitorRevision | undefined {
  if (!detail) return undefined;
  return (
    detail.revisions.find((revision) => revision.id === detail.monitor.draft_revision_id) ??
    detail.revisions.find((revision) => revision.id === detail.monitor.active_revision_id) ??
    detail.revisions[0]
  );
}

export function publishedRevision(detail: MonitorDetail | undefined): MonitorRevision | undefined {
  if (!detail) return undefined;
  return detail.revisions.find((revision) => revision.id === detail.monitor.active_revision_id);
}

export function scheduleSeconds(schedule: MonitorSchedule | undefined): number | undefined {
  if (!schedule) return undefined;
  if (schedule.kind === 'interval') return schedule.every_seconds;
  if (schedule.kind === 'heartbeat') return schedule.expected_seconds;
  return undefined;
}

export function scheduleText(schedule: MonitorSchedule | undefined): string {
  if (!schedule) return '—';
  if (schedule.kind === 'cron') return schedule.expression;
  const seconds = scheduleSeconds(schedule) ?? 0;
  if (seconds < 60) return `${seconds}s`;
  if (seconds % 3600 === 0) return `${seconds / 3600}h`;
  return `${seconds / 60}m`;
}

export function resultDurationMicros(result: SyntheticResult | undefined): number | undefined {
  if (!result) return undefined;
  const latestAttempt = result.attempts.at(-1);
  return latestAttempt?.timing.total_micros ?? Math.max(0, result.finished_at - result.started_at);
}

export function successRate(results: SyntheticResult[]): number | undefined {
  const decided = operationalResults(results).filter(
    (result) => result.outcome !== 'unknown' && result.outcome !== 'skipped',
  );
  if (decided.length === 0) return undefined;
  return decided.filter((result) => result.outcome === 'healthy').length / decided.length;
}

export function operationalResults(results: SyntheticResult[]): SyntheticResult[] {
  return results.filter((result) => !result.is_test);
}

export function percentile(values: number[], quantile: number): number | undefined {
  const finite = values.filter(Number.isFinite).sort((left, right) => left - right);
  if (finite.length === 0) return undefined;
  const index = Math.min(finite.length - 1, Math.max(0, Math.ceil(finite.length * quantile) - 1));
  return finite[index];
}

export function formatDuration(micros: number | undefined): string {
  if (micros === undefined || !Number.isFinite(micros)) return '—';
  const millis = micros / 1000;
  if (millis < 1) return `${Math.round(micros)} µs`;
  if (millis < 1000) return `${millis < 10 ? millis.toFixed(1) : Math.round(millis)} ms`;
  return `${(millis / 1000).toFixed(millis < 10_000 ? 2 : 1)} s`;
}

export function formatPercent(value: number | undefined): string {
  if (value === undefined || !Number.isFinite(value)) return '—';
  return `${(value * 100).toFixed(value >= 0.9995 ? 2 : 1)}%`;
}

export function formatTimestamp(micros: number | undefined, locale = 'en-US'): string {
  if (!micros) return '—';
  return new Intl.DateTimeFormat(locale, {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  }).format(new Date(micros / 1000));
}

export function formatRelativeTimestamp(micros: number | undefined, locale = 'en-US'): string {
  if (!micros) return '—';
  const seconds = Math.round((micros / 1000 - Date.now()) / 1000);
  const formatter = new Intl.RelativeTimeFormat(locale, { numeric: 'auto' });
  if (Math.abs(seconds) < 60) return formatter.format(seconds, 'second');
  const minutes = Math.round(seconds / 60);
  if (Math.abs(minutes) < 60) return formatter.format(minutes, 'minute');
  const hours = Math.round(minutes / 60);
  if (Math.abs(hours) < 24) return formatter.format(hours, 'hour');
  return formatter.format(Math.round(hours / 24), 'day');
}

export function detailToInput(detail: MonitorDetail, clone = false): CreateMonitorInput | undefined {
  const revision = activeRevision(detail);
  if (!revision) return undefined;
  return {
    name: clone ? `${detail.monitor.name} copy` : detail.monitor.name,
    description: detail.monitor.description,
    spec: revision.spec,
    schedule: revision.schedule,
    timeout_millis: revision.timeout_millis,
    max_retries: revision.max_retries,
    consecutive_failures: revision.consecutive_failures,
    consecutive_recoveries: revision.consecutive_recoveries,
    freshness_seconds: revision.freshness_seconds,
    location_policy: revision.location_policy,
    location_ids: revision.location_ids,
    ...(detail.monitor.team_id ? { team_id: detail.monitor.team_id } : {}),
    tags: detail.monitor.tags,
    ...(revision.escalation_policy_id
      ? { escalation_policy_id: revision.escalation_policy_id }
      : {}),
    alert_on_degraded: revision.alert_on_degraded,
  };
}

export function clientId(): string {
  return typeof crypto !== 'undefined' && 'randomUUID' in crypto
    ? crypto.randomUUID()
    : `synthetic-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

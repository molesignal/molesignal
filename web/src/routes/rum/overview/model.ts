import type {
  ErrorRow,
  ExperienceGrade,
  SessionRow,
  WebVitalsPoint,
} from '@/api/rum';

export const ALL = '__all__';

export interface RumScope {
  application: string;
  environment: string;
  version: string;
  country: string;
  device: string;
}

export interface OverviewMetrics {
  users: number;
  sessions: number;
  errorFreeRate: number;
  lcpP75: number;
  inpP75: number;
  clsP75: number;
}

export interface SlowPage {
  page: string;
  p75: number;
  sessions: number;
  errorRate: number;
  grade: ExperienceGrade;
}

export interface DimensionShare {
  label: string;
  count: number;
  share: number;
}

export function initialScope(): RumScope {
  return {
    application: ALL,
    environment: ALL,
    version: ALL,
    country: ALL,
    device: ALL,
  };
}

export function applySessionScope(
  rows: SessionRow[],
  scope: RumScope,
): SessionRow[] {
  return rows.filter(
    (row) =>
      (scope.application === ALL || row.application === scope.application) &&
      (scope.environment === ALL || row.environment === scope.environment) &&
      (scope.version === ALL || row.version === scope.version) &&
      (scope.country === ALL || row.country === scope.country) &&
      (scope.device === ALL || row.device === scope.device),
  );
}

export function applyVitalScope(
  rows: WebVitalsPoint[],
  scope: RumScope,
): WebVitalsPoint[] {
  return rows.filter(
    (row) =>
      (scope.application === ALL || row.application === scope.application) &&
      (scope.environment === ALL || row.environment === scope.environment) &&
      (scope.version === ALL || row.version === scope.version) &&
      (scope.country === ALL || row.country === scope.country) &&
      (scope.device === ALL || row.device === scope.device),
  );
}

export function overviewMetrics(
  sessions: SessionRow[],
  vitals: WebVitalsPoint[],
): OverviewMetrics {
  const errorFree = sessions.filter(
    (session) =>
      (session.error_count ?? 0) === 0 && session.failed_request_count === 0,
  ).length;
  const lcp = metricValues(vitals, 'lcp_ms', sessions);
  const inp = metricValues(vitals, 'inp_ms', sessions);
  const cls = metricValues(vitals, 'cls', sessions);
  return {
    users: new Set(
      sessions
        .map((session) => session.user_id ?? session.session_id)
        .filter(Boolean),
    ).size,
    sessions: sessions.length,
    errorFreeRate: sessions.length === 0 ? 0 : errorFree / sessions.length,
    lcpP75: percentile(lcp, 0.75),
    inpP75: percentile(inp, 0.75),
    clsP75: percentile(cls, 0.75),
  };
}

export function slowestPages(
  points: WebVitalsPoint[],
  sessions: SessionRow[],
): SlowPage[] {
  const groups = new Map<string, WebVitalsPoint[]>();
  for (const point of points) {
    const page = point.page ?? '—';
    groups.set(page, [...(groups.get(page) ?? []), point]);
  }
  return Array.from(groups.entries())
    .map(([page, values]) => {
      const lcp = percentile(
        values
          .map((value) => value.lcp_ms)
          .filter((value): value is number => value !== undefined),
        0.75,
      );
      const related = sessions.filter(
        (session) =>
          session.last_page === page ||
          session.landing_page === page ||
          session.journey.includes(page),
      );
      const withErrors = related.filter(
        (session) =>
          (session.error_count ?? 0) > 0 || session.failed_request_count > 0,
      ).length;
      return {
        page,
        p75: lcp,
        sessions: new Set(
          values
            .map((value) => value.session_id)
            .filter((value): value is string => Boolean(value)),
        ).size,
        errorRate: related.length === 0 ? 0 : withErrors / related.length,
        grade: lcpGrade(lcp),
      };
    })
    .filter((page) => page.p75 > 0)
    .sort((left, right) => right.p75 - left.p75);
}

export function frequentErrors(errors: ErrorRow[]): ErrorRow[] {
  return [...errors]
    .sort((left, right) => right.users - left.users || right.count - left.count)
    .slice(0, 5);
}

export function dimensionShares(
  sessions: SessionRow[],
  fields: Array<'browser' | 'device'>,
): DimensionShare[] {
  const counts = new Map<string, number>();
  for (const session of sessions) {
    const label =
      fields.map((field) => session[field]).filter(Boolean).join(' · ') || '—';
    counts.set(label, (counts.get(label) ?? 0) + 1);
  }
  return shareRows(counts, sessions.length);
}

export function regionShares(sessions: SessionRow[]): DimensionShare[] {
  const counts = new Map<string, number>();
  for (const session of sessions) {
    const label = session.country ?? '—';
    counts.set(label, (counts.get(label) ?? 0) + 1);
  }
  return shareRows(counts, sessions.length);
}

export function valuesFor(
  rows: SessionRow[],
  field: 'application' | 'environment' | 'version' | 'country' | 'device',
): string[] {
  return Array.from(
    new Set(
      rows
        .map((row) => row[field])
        .filter((value): value is string => Boolean(value)),
    ),
  ).sort();
}

export function scopeOptions(values: string[], allLabel: string) {
  return [
    { value: ALL, label: allLabel },
    ...values.map((value) => ({ value, label: value })),
  ];
}

export function percentile(values: number[], fraction: number): number {
  if (values.length === 0) return 0;
  const sorted = [...values].sort((left, right) => left - right);
  return (
    sorted[
      Math.min(sorted.length - 1, Math.ceil(sorted.length * fraction) - 1)
    ] ?? 0
  );
}

function metricValues(
  vitals: WebVitalsPoint[],
  key: 'lcp_ms' | 'inp_ms' | 'cls',
  sessions: SessionRow[],
): number[] {
  const values = vitals
    .map((point) => point[key])
    .filter((value): value is number => value !== undefined);
  if (values.length > 0) return values;
  return sessions
    .map((session) => session[key])
    .filter((value): value is number => value !== undefined);
}

function lcpGrade(value: number): ExperienceGrade {
  if (value > 4_000) return 'poor';
  if (value > 2_500) return 'needs_improvement';
  return value > 0 ? 'good' : 'unknown';
}

function shareRows(
  counts: Map<string, number>,
  total: number,
): DimensionShare[] {
  return Array.from(counts.entries())
    .map(([label, count]) => ({
      label,
      count,
      share: total === 0 ? 0 : count / total,
    }))
    .sort((left, right) => right.count - left.count)
    .slice(0, 6);
}

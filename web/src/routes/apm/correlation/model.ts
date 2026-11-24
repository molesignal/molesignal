import type { CorrelationGrade, CorrelationGrades, CorrelationReport, ServiceCorrelation } from '@/api/apmCorrelation';

/** The ways a service ties to other data, in the order their dots are drawn. */
export const CORRELATION_ASPECTS = ['traceIds', 'resource', 'metrics', 'changes'] as const satisfies ReadonlyArray<keyof CorrelationGrades>;

export type CorrelationAspect = (typeof CORRELATION_ASPECTS)[number];

const GRADE_POINTS: Record<CorrelationGrade, number> = { good: 2, partial: 1, missing: 0, unknown: 0 };

/** How well a service ties to other data all told, for sorting: two a way it ties well, one a way it ties in part. */
export function correlationScore(grades: CorrelationGrades): number {
  return CORRELATION_ASPECTS.reduce((sum, aspect) => sum + GRADE_POINTS[grades[aspect]], 0);
}

function identityKey(service: { namespace: string; name: string; environment: string }): string {
  return `${service.namespace}/${service.name}/${service.environment}`;
}

/** Looks up a service's correlation by its identity. */
export function correlationLookup(report: CorrelationReport | undefined): (service: { namespace: string; name: string; environment: string | null }) => ServiceCorrelation | null {
  const byKey = new Map((report?.services ?? []).map((entry) => [identityKey(entry.service), entry]));

  return (service) => (service.environment === null ? null : (byKey.get(identityKey({ ...service, environment: service.environment })) ?? null));
}

/** The share of a service's log lines that carry a trace id; `null` without lines. */
export function tracedShare(entry: ServiceCorrelation): number | null {
  return entry.logs.lines > 0 ? entry.logs.traced / entry.logs.lines : null;
}

/** Something the correlation report asks to be fixed or suggests. */
export type CorrelationFinding =
  | { key: 'untraced' | 'partly_traced' | 'unplaced' | 'no_changes'; tone: 'warning' | 'suggestion'; names: string[] }
  | { key: 'unmatched'; tone: 'suggestion'; names: string[] };

/** Service names once each, sorted, of the entries that match. */
function namesOf(services: readonly ServiceCorrelation[], keep: (entry: ServiceCorrelation) => boolean): string[] {
  return [...new Set(services.filter(keep).map((entry) => entry.service.name))].sort();
}

/**
 * What the report asks of the organization, the fixes first: services whose
 * logs carry no trace id or carry it only in part, services that do not say
 * where they run, names in logs and metrics no service answers to, and
 * services that reported no change over the week.
 */
export function correlationFindings(report: CorrelationReport | undefined): CorrelationFinding[] {
  if (!report) return [];

  const services = report.services;
  const findings: CorrelationFinding[] = [
    { key: 'untraced', tone: 'warning', names: namesOf(services, (entry) => entry.logs.state === 'reported' && entry.grades.traceIds === 'missing') },
    { key: 'partly_traced', tone: 'warning', names: namesOf(services, (entry) => entry.grades.traceIds === 'partial') },
    { key: 'unplaced', tone: 'warning', names: namesOf(services, (entry) => entry.resource.instance && !entry.resource.placement) },
    { key: 'unmatched', tone: 'suggestion', names: [...new Set(report.unmatched.map((name) => name.name))] },
    { key: 'no_changes', tone: 'suggestion', names: namesOf(services, (entry) => entry.grades.changes === 'missing') },
  ];

  return findings.filter((finding) => finding.names.length > 0);
}

/** A few names and how many more there are. */
export function someNames(names: readonly string[], shown = 3): { shown: string[]; more: number } {
  return { shown: names.slice(0, shown), more: Math.max(0, names.length - shown) };
}

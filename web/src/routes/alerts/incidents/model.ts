import type { Incident, Severity } from '@/types/alerting';

/** Severities from the worst down: the order they are counted, listed and drawn in. */
export const SEVERITIES_WORST_FIRST: readonly Severity[] = ['critical', 'error', 'warning', 'info'];

/** The colour of a severity as a mark: a share of a bar, a dot. */
export const SEVERITY_COLOR: Record<Severity, string> = {
  critical: 'var(--red)',
  error: 'var(--orange)',
  warning: 'var(--yellow)',
  info: 'var(--blue)',
};

const SEVERITY_RANK: Record<Severity, number> = { info: 0, warning: 1, error: 2, critical: 3 };

export function isResolvedIncident(incident: Incident): boolean {
  return incident.status === 'resolved' || incident.status === 'closed';
}

/** The service an incident is about, as the rows and the search name it. */
export function incidentService(incident: Incident): string | undefined {
  return incident.affected_services?.[0] ?? incident.labels?.service ?? incident.labels?.svc;
}

/** Every service the incidents touch, each once, in the order they are first met. */
export function affectedServices(incidents: readonly Incident[]): string[] {
  const services = new Set<string>();

  incidents.forEach((incident) => {
    [...(incident.affected_services ?? []), incident.labels?.service, incident.labels?.svc].forEach((service) => {
      if (service) services.add(service);
    });
  });

  return [...services];
}

export function severityCounts(incidents: readonly Incident[]): Record<Severity, number> {
  const counts: Record<Severity, number> = { critical: 0, error: 0, warning: 0, info: 0 };

  incidents.forEach((incident) => {
    counts[incident.severity] += 1;
  });

  return counts;
}

/** What needs a person first: the worst, and among equals the one that has burnt longest. */
export function byUrgency(left: Incident, right: Incident): number {
  return SEVERITY_RANK[right.severity] - SEVERITY_RANK[left.severity] || left.created_at - right.created_at;
}

/** How long an incident has lasted, or lasted until it recovered, in seconds. */
export function incidentDurationSecs(incident: Incident, nowMicros: number): number {
  const end = incident.resolved_at ?? nowMicros;

  return Math.max(0, Math.round((end - incident.created_at) / 1_000_000));
}

/** Where an incident's runbook is, if its rule or the incident itself names one. */
export function incidentRunbook(
  incident: Incident,
  ruleAnnotations: Record<string, string> | undefined,
): string | undefined {
  const url = incident.annotations?.runbook_url ?? incident.annotations?.runbook ?? ruleAnnotations?.runbook_url;

  // A runbook is opened in the browser: only an address it can open is one.
  return url && /^https?:\/\//i.test(url) ? url : undefined;
}

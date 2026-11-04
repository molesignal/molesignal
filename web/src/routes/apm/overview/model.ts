import type { HealthTrendPoint, ServiceHealth, ServiceIdentity } from '@/api/apm';
import type { ReadoutTone } from '@/shell/readout/Readout';
import type { Incident } from '@/types/alerting';

/** Each state of a service in its own ink, as a word. */
export const HEALTH_TEXT: Record<ServiceHealth, string> = {
  critical: 'text-[var(--red-ink)]',
  warning: 'text-[var(--yellow-ink)]',
  healthy: 'text-[var(--green-ink)]',
  no_traffic: 'text-tx-3',
};

/** Each state of a service as a fill: a dot, or a slice of a bar. */
export const HEALTH_FILL: Record<ServiceHealth, string> = {
  critical: 'var(--red)',
  warning: 'var(--yellow)',
  healthy: 'var(--green)',
  no_traffic: 'var(--tx-4)',
};

export const HEALTH_ORDER: readonly ServiceHealth[] = ['critical', 'warning', 'healthy', 'no_traffic'];

/** The Apdex bands: excellent and good read as good, fair as a warning, poor and worse as danger. */
export function apdexTone(score: number | null | undefined): ReadoutTone | undefined {
  if (score === null || score === undefined) return undefined;
  if (score >= 0.85) return 'good';
  if (score >= 0.7) return 'warn';
  return 'danger';
}

/** The lines the health of a service is told by: 2% of errors is a warning, 10% critical. */
export function errorRateTone(rate: number, requests: number): ReadoutTone | undefined {
  if (requests === 0) return undefined;
  if (rate >= 0.1) return 'danger';
  if (rate >= 0.02) return 'warn';
  return 'good';
}

/** The labels an alert names a service by, the OpenTelemetry name first. */
const SERVICE_LABELS = ['service.name', 'service_name', 'service'] as const;

/** The service an incident is about, from its labels or the services it says it touched. */
export function incidentService(incident: Pick<Incident, 'labels' | 'affectedServices'>): string | null {
  for (const key of SERVICE_LABELS) {
    const value = incident.labels?.[key]?.trim();
    if (value) return value;
  }
  return incident.affectedServices?.find((name) => name.trim())?.trim() ?? null;
}

/** The incidents still going on that name a service, the most severe and then the newest first. */
export function serviceIncidents(incidents: readonly Incident[]): Array<{ incident: Incident; service: string }> {
  const severity = { critical: 0, error: 1, warning: 2, info: 3 } as const;

  return incidents
    .filter((incident) => incident.status === 'open' || incident.status === 'acknowledged')
    .flatMap((incident) => {
      const service = incidentService(incident);
      return service ? [{ incident, service }] : [];
    })
    .sort(
      (left, right) =>
        severity[left.incident.severity] - severity[right.incident.severity] || right.incident.createdAt - left.incident.createdAt,
    );
}

/** The health trend as one series per state, for a stacked bar chart. */
export function healthSeries(trend: readonly HealthTrendPoint[], label: (health: ServiceHealth) => string) {
  const timestamps = trend.map((point) => point.bucketAt);

  return HEALTH_ORDER.map((health) => ({
    id: health,
    name: label(health),
    timestamps,
    data: trend.map((point) => point.counts[health]),
    color: HEALTH_FILL[health],
  }));
}

/** A service's key in a list. */
export function serviceRowKey(service: ServiceIdentity): string {
  return `${service.namespace}/${service.name}/${service.environment}`;
}

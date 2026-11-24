import type { TraceFilter } from '@/api/web';
import { tracesHref } from '@/routes/apm/endpointFilters';
import type { RcaEvidence } from '@/types/alerting';

/** What the backend says of failed requests without a value of the field. */
const NO_VALUE = '(no value)';

/** Where a piece of evidence opens; `null` when it says too little to open anywhere. */
export function evidenceHref(piece: RcaEvidence): string | null {
  const link = piece.link;
  if (!link) return null;
  if (link.route?.startsWith('/') && !link.route.startsWith('//')) return link.route;
  if (link.traceId) return `/traces/${encodeURIComponent(link.traceId)}`;
  if (link.incidentId) return `/alerts/incidents/${encodeURIComponent(link.incidentId)}`;

  if (link.fingerprint && link.namespace && link.environment) {
    const params = new URLSearchParams({ namespace: link.namespace, environment: link.environment, error: link.fingerprint });
    return `/apm/errors?${params.toString()}`;
  }

  if (piece.service && link.namespace && link.environment) {
    const params = new URLSearchParams({ namespace: link.namespace, environment: link.environment });
    return `/apm/services/${encodeURIComponent(piece.service)}?${params.toString()}`;
  }

  // The failed requests with the value they crowd into, on the Traces page.
  if (piece.kind === 'attribute_skew' && piece.subject && link.from !== undefined && link.to !== undefined) {
    const value = piece.detail && piece.detail !== NO_VALUE ? piece.detail : null;
    const filters: TraceFilter[] = [
      { field: 'status_code', op: '=', value: 'ERROR' },
      ...(value ? [{ field: piece.subject, op: '=' as const, value }] : []),
    ];
    return `${tracesHref(filters, { from: link.from, to: link.to })}&tab=red`;
  }

  if (link.stream && link.from !== undefined && link.to !== undefined) {
    const from = new Date(link.from / 1_000).toISOString();
    const to = new Date(link.to / 1_000).toISOString();
    const params = new URLSearchParams({ stream: link.stream, from, to, time: `${from}..${to}` });
    return `/logs?${params.toString()}`;
  }

  return null;
}

/** The figure of an insight in the unit of what moved, the metric being the part after the dot of its detail. */
export function insightFigure(detail: string | undefined, value: number, locale: string): string {
  switch (detail?.split('.')[1]) {
    case 'error_rate':
      return value.toLocaleString(locale, { style: 'percent', maximumFractionDigits: 2 });
    case 'p95':
      return `${(value / 1_000).toLocaleString(locale, { maximumFractionDigits: 1 })} ms`;
    case 'throughput':
      return `${value.toLocaleString(locale, { maximumFractionDigits: 2 })} req/s`;
    case 'log_errors':
      return `${value.toLocaleString(locale, { maximumFractionDigits: 1 })}/min`;
    default:
      return value.toLocaleString(locale, { maximumFractionDigits: 2 });
  }
}

/** The labels a summary cites, such as `[E2]`, in the order first cited. */
export function citedLabels(summary: string): string[] {
  return [...new Set([...summary.matchAll(/\[(E\d+)\]/g)].map((match) => match[1]!))];
}

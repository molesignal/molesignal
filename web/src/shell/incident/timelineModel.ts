import type { Incident, TimelineEntry } from '@/types/alerting';

/** Whose doing a row of the timeline is: the alert itself, a notification, a person, or what was written. */
export type TimelineCategory = 'alert' | 'notice' | 'people' | 'notes';

export function categoryOfEntry(entry: Pick<TimelineEntry, 'kind' | 'actor'>): TimelineCategory {
  if (entry.kind === 'note' || entry.kind === 'status_update') return 'notes';
  return entry.actor ? 'people' : 'alert';
}

/** One thing an alert pipeline changed: `severity`, `labels.team` and the like. */
export interface PipelineChange {
  field: string;
  from?: string | undefined;
  to?: string | undefined;
}

/** What a `pipeline_processed` row of the timeline holds. */
export interface PipelineTraceDetail {
  pipeline: string;
  changes: PipelineChange[];
  more: number;
  error: string | null;
}

export function pipelineTraceOf(detail: unknown): PipelineTraceDetail {
  const raw = (detail ?? {}) as Record<string, unknown>;

  return {
    pipeline: typeof raw.pipeline === 'string' ? raw.pipeline : '',
    changes: Array.isArray(raw.changes) ? (raw.changes as PipelineChange[]) : [],
    more: typeof raw.more === 'number' ? raw.more : 0,
    error: typeof raw.error === 'string' ? raw.error : null,
  };
}

/**
 * A change in as few words as say it: `error → critical`, `+team=payments`,
 * `-host_name=web-01`, with labels and annotations under their own group.
 */
export function pipelineChangeText(change: PipelineChange): { group: string; text: string } {
  const [group, ...rest] = change.field.split('.');
  const key = rest.join('.');
  const nested = group === 'labels' || group === 'annotations';

  if (!nested) return { group: change.field, text: `${change.from ?? ''} → ${change.to ?? ''}` };
  if (change.from === undefined) return { group: group ?? '', text: `+${key}=${change.to ?? ''}` };
  if (change.to === undefined) return { group: group ?? '', text: `-${key}=${change.from}` };

  return { group: group ?? '', text: `${key}: ${change.from} → ${change.to}` };
}

export interface IncidentPhase {
  key: 'fired' | 'acknowledged' | 'resolved';
  /** How long after it fired, in micros; absent while that has not happened. */
  after: number | null;
}

/** The three moments every incident is measured by. A step skipped (resolved unanswered) stays empty. */
export function incidentPhases(incident: Pick<Incident, 'createdAt' | 'acknowledgedAt' | 'resolvedAt'>): IncidentPhase[] {
  const after = (at: number | undefined) => (at ? Math.max(0, at - incident.createdAt) : null);

  return [
    { key: 'fired', after: 0 },
    { key: 'acknowledged', after: after(incident.acknowledgedAt) },
    { key: 'resolved', after: after(incident.resolvedAt) },
  ];
}

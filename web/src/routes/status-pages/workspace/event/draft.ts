import type {
  IncidentImpact,
  PublicIncidentStatus,
  StatusPageIncident,
  StatusPageIncidentInput,
  StatusPageIncidentKind,
} from '@/api/statusPages';

import { INCIDENT_STATUSES } from '../../model';

/** What the event form holds while it is being written. */
export interface EventDraft {
  title: string;
  sourceIncidentId: string;
  impact: IncidentImpact;
  status: PublicIncidentStatus;
  message: string;
  componentIds: string[];
  startedAt: string;
  /** Minutes from publishing by which the next update is promised; null promises none. */
  nextUpdateMinutes: number | null;
  /** Whether publishing tells the page's subscribers. */
  notify: boolean;
}

/** The promises of a next update an operator picks from, in minutes. */
export const NEXT_UPDATE_CHOICES = [15, 30, 60, 120, 240, 1_440] as const;

export function initialDraft(kind: StatusPageIncidentKind, event: StatusPageIncident | null): EventDraft {
  return {
    title: event?.title ?? '',
    sourceIncidentId: event?.source_incident_id ?? '',
    impact: event?.impact ?? (kind === 'maintenance' ? 'maintenance' : 'major'),
    status: event?.status ?? (kind === 'maintenance' ? 'scheduled' : 'investigating'),
    message: event?.draft_message ?? '',
    componentIds: event?.component_ids ?? [],
    startedAt: toLocalDateTime(event?.started_at ?? (Date.now() + 60 * 60_000) * 1_000),
    nextUpdateMinutes: null,
    notify: true,
  };
}

export function eventInput(
  kind: StatusPageIncidentKind,
  draft: EventDraft,
  publicationState: 'draft' | 'published',
  now = Date.now(),
): StatusPageIncidentInput {
  return {
    source_incident_id: kind === 'incident' ? draft.sourceIncidentId || null : null,
    kind,
    title: draft.title.trim(),
    impact: kind === 'maintenance' ? 'maintenance' : draft.impact,
    status: draft.status,
    publication_state: publicationState,
    message: draft.message.trim() || null,
    component_ids: draft.componentIds,
    ...(kind === 'maintenance' ? { started_at: new Date(draft.startedAt).getTime() * 1_000 } : {}),
    // Who is told, and what is promised, is decided when the event goes out.
    ...(publicationState === 'published'
      ? {
          notify: draft.notify,
          next_update_at: nextUpdateAt(kind, draft.status, draft.nextUpdateMinutes, now),
        }
      : {}),
  };
}

/** Whether an update in this status can promise the next one: only while the event goes on. */
export function promisesNextUpdate(kind: StatusPageIncidentKind, status: PublicIncidentStatus): boolean {
  return status !== 'scheduled' && !isTerminalStatus(kind, status);
}

/** The moment a promise of `minutes` made `now` points at, in microseconds. */
export function nextUpdateAt(
  kind: StatusPageIncidentKind,
  status: PublicIncidentStatus,
  minutes: number | null,
  now = Date.now(),
): number | null {
  if (minutes === null || !promisesNextUpdate(kind, status)) return null;
  return (now + minutes * 60_000) * 1_000;
}

export function nextStatuses(event: StatusPageIncident): PublicIncidentStatus[] {
  if (event.kind === 'maintenance') {
    if (event.status === 'scheduled') return ['scheduled', 'in_progress', 'cancelled'];
    if (event.status === 'in_progress') return ['in_progress', 'completed', 'cancelled'];
    return [];
  }
  const index = INCIDENT_STATUSES.indexOf(event.status);
  if (event.status === 'monitoring') return ['monitoring', 'in_progress', 'resolved'];
  return index >= 0 ? INCIDENT_STATUSES.slice(index) : [];
}

export function isTerminal(event: StatusPageIncident): boolean {
  return isTerminalStatus(event.kind, event.status);
}

function isTerminalStatus(kind: StatusPageIncidentKind, status: PublicIncidentStatus): boolean {
  return kind === 'incident'
    ? status === 'resolved'
    : status === 'completed' || status === 'cancelled';
}

/** The latest promise of a next update, while the event goes on. */
export function promisedNextUpdate(event: StatusPageIncident): number | null {
  if (isTerminal(event)) return null;
  const latest = [...event.updates].sort(
    (left, right) => right.created_at - left.created_at || right.id.localeCompare(left.id),
  )[0];
  return latest?.next_update_at ?? null;
}

export function toLocalDateTime(micros: number): string {
  const date = new Date(Math.floor(micros / 1_000));
  const offset = date.getTimezoneOffset() * 60_000;
  return new Date(date.getTime() - offset).toISOString().slice(0, 16);
}

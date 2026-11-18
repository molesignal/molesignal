import * as React from 'react';

import { useUiPreference } from '@/shell/uiPreference/useUiPreference';

/** What the workbench can open again. */
export const VISIT_KINDS = [
  'dashboard',
  'service',
  'incident',
  'saved_view',
  'check',
  'status_page',
] as const;

export type VisitKind = (typeof VISIT_KINDS)[number];

export interface RecentVisit {
  kind: VisitKind;
  id: string;
  label: string;
  /** Where it opens, inside the product. */
  path: string;
  /** Microseconds since the epoch. */
  visitedAt: number;
}

/** The most places kept: the six the workbench shows and a few behind them. */
export const MAX_RECENT_VISITS = 12;

function isVisit(value: unknown): value is RecentVisit {
  if (!value || typeof value !== 'object') return false;
  const visit = value as Record<string, unknown>;

  return (
    VISIT_KINDS.includes(visit.kind as VisitKind) &&
    typeof visit.id === 'string' &&
    visit.id !== '' &&
    typeof visit.label === 'string' &&
    visit.label !== '' &&
    typeof visit.path === 'string' &&
    visit.path.startsWith('/') &&
    !visit.path.startsWith('//') &&
    typeof visit.visitedAt === 'number'
  );
}

/** What is kept, as far as it is a list of visits this build knows; anything else is left out. */
export function parseRecentVisits(raw: unknown): { visits: RecentVisit[] } | null {
  if (!raw || typeof raw !== 'object') return null;
  const visits = (raw as { visits?: unknown }).visits;

  return Array.isArray(visits) ? { visits: visits.filter(isVisit).slice(0, MAX_RECENT_VISITS) } : null;
}

/** `visit` first, the same place taken out further down, the list cut to its length. */
export function withVisit(visits: readonly RecentVisit[], visit: RecentVisit): RecentVisit[] {
  return [visit, ...visits.filter((kept) => kept.kind !== visit.kind || kept.id !== visit.id)].slice(
    0,
    MAX_RECENT_VISITS,
  );
}

/** A label is kept to the length the server takes. */
const MAX_LABEL_CHARS = 200;
/** A longer address (a saved query's whole statement) is not kept: the server would refuse the list. */
const MAX_PATH_CHARS = 2_048;

export function useRecentVisits() {
  return useUiPreference('workbench.recent', 'visits', parseRecentVisits);
}

/** Keeps a place among the recent visits as it is opened; nothing while the server's list is unknown. */
export function useVisitRecorder(): (visit: Omit<RecentVisit, 'visitedAt'>) => void {
  const recent = useRecentVisits();
  const recentRef = React.useRef(recent);
  recentRef.current = recent;

  return React.useCallback((visit) => {
    const current = recentRef.current;
    if (!current.settled || visit.path.length > MAX_PATH_CHARS) return;

    current.save({
      visits: withVisit(current.value?.visits ?? [], {
        ...visit,
        label: visit.label.slice(0, MAX_LABEL_CHARS),
        visitedAt: Date.now() * 1000,
      }),
    });
  }, []);
}

/**
 * Keeps the page being looked at among the workbench's recent visits, once it is known what it is:
 * pass `null` while it loads. It waits for the server's list, so that nothing kept elsewhere is
 * written over.
 */
export function useRecordVisit(visit: Omit<RecentVisit, 'visitedAt'> | null) {
  const recent = useRecentVisits();
  const recentRef = React.useRef(recent);
  recentRef.current = recent;
  const identity = visit ? JSON.stringify([visit.kind, visit.id, visit.label, visit.path]) : null;

  React.useEffect(() => {
    const current = recentRef.current;
    if (!visit || !current.settled || visit.path.length > MAX_PATH_CHARS) return;

    const visits = current.value?.visits ?? [];
    const first = visits[0];
    // Opened again right after: the list already says so.
    if (first && first.kind === visit.kind && first.id === visit.id && first.label === visit.label) return;

    current.save({
      visits: withVisit(visits, {
        ...visit,
        label: visit.label.slice(0, MAX_LABEL_CHARS),
        visitedAt: Date.now() * 1000,
      }),
    });
    // `identity` stands for `visit`: a new object of the same place is not a new visit.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [identity, recent.settled]);
}

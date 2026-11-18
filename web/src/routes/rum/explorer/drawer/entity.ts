import * as rumApi from '@/api/rum';
import type { RumRow } from '@/api/rum';

import { type RumTab, rowId } from '../model';

/** A second either side of the row: a row is dated at the moment it happened, and read by its id. */
const LOOKUP_PADDING_MICROS = 1_000_000;

export const NO_ROW: RumRow = { timestamp: 0 };

export function isPresent(value: unknown): boolean {
  return value !== null && value !== undefined && value !== '';
}

/** How a row is read whole: by its id, over where it happened. The head of the drawer and its body read it alike, and so once. */
export function entityOptions(tab: RumTab, listed: RumRow, span: { from: number; to: number } | undefined) {
  const id = rowId(tab, listed);
  const lookup = span ?? {
    from: listed.timestamp - LOOKUP_PADDING_MICROS,
    to: listed.timestamp + LOOKUP_PADDING_MICROS,
  };

  return {
    queryKey: ['rum', 'entity', tab, id, lookup.from, lookup.to],
    queryFn: ({ signal }: { signal: AbortSignal }) => rumApi.entity(tab, id, lookup, signal),
    enabled: id !== '',
    staleTime: 60_000,
    retry: false,
  };
}

/** The page of the session a row belongs to; none for an error of a session that was not kept, which is kept alone. */
export function sessionHref(row: RumRow): string | null {
  const id = row['session.id'];
  if (row['retention.filter'] === 'errors') return null;
  return typeof id === 'string' && id !== '' ? `/rum/sessions/view/${encodeURIComponent(id)}` : null;
}

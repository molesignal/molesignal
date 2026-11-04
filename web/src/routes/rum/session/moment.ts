import type { RumRow } from '@/api/rum';

import { sessionWindow } from './model';

/** How far beyond the session the range of a link reaches, so that a clock a little off still finds it. */
const MARGIN_MS = 60_000;

/**
 * The address of a session, to open it where a person wants it opened: at a moment of its replay
 * (microseconds since the epoch), inside a playlist, and with the stretch of time the session ran in,
 * so that a link to an old session still finds it.
 */
export function sessionPath(
  id: string,
  options: {
    session?: RumRow | null | undefined;
    /** The moment to open the replay at; the beginning when none. */
    at?: number | null | undefined;
    /** The playlist the session is gone through in. */
    playlist?: string | null | undefined;
  } = {},
): string {
  const params = new URLSearchParams();
  const during = sessionWindow(options.session);

  if (options.playlist) params.set('playlist', options.playlist);

  if (typeof options.at === 'number' && Number.isFinite(options.at) && options.at > 0) {
    params.set('at', String(Math.round(options.at)));
  }

  if (during) {
    params.set('from', new Date(Date.parse(during.from) - MARGIN_MS).toISOString());
    params.set('to', new Date(Date.parse(during.to) + MARGIN_MS).toISOString());
  }

  const query = params.toString();

  return `/rum/sessions/view/${encodeURIComponent(id)}${query ? `?${query}` : ''}`;
}

/** The place in a list of sessions: where the session is, and the ones on either side of it. */
export function neighbours<T extends { sessionId: string }>(
  entries: readonly T[],
  current: string,
): { index: number; total: number; previous: T | null; next: T | null } | null {
  const index = entries.findIndex((entry) => entry.sessionId === current);

  if (index < 0) return null;

  return {
    index,
    total: entries.length,
    previous: entries[index - 1] ?? null,
    next: entries[index + 1] ?? null,
  };
}

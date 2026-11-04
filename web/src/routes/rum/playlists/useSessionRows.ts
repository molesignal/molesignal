import { useQuery } from '@tanstack/react-query';

import * as rumApi from '@/api/rum';
import type { RumRow } from '@/api/rum';
import { useAuthStore } from '@/stores/auth';

/** The most values an `in` filter holds. */
const CHUNK = 100;

/** How far back a playlist looks for its sessions: as long as the data is kept, by default. */
const LOOKBACK_MICROS = 90 * 24 * 3_600 * 1_000_000;

const COLUMNS = [
  'session.id',
  'session.duration',
  'session.error.count',
  'session.frustration.count',
  'session.crash.count',
  'session.has_replay',
  'session.landing_view.url_path',
  'session.exit_view.url_path',
  'usr.id',
  'usr.name',
  'usr.email',
  'usr.anonymous_id',
  'env',
  'version',
];

/**
 * The rows of the sessions a list holds, by their ids, in the order of `ids`: none for a session
 * the streams no longer have. A list longer than an `in` filter takes is read in parts.
 */
export function useSessionRows(ids: readonly string[]) {
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');
  const key = ids.join('\u0000');

  return useQuery({
    queryKey: ['rum', 'playlists', orgId, 'rows', key],
    queryFn: async (): Promise<Map<string, RumRow>> => {
      const to = Date.now() * 1_000 + 60_000_000;
      const chunks = Array.from({ length: Math.ceil(ids.length / CHUNK) }, (_, at) => ids.slice(at * CHUNK, (at + 1) * CHUNK));

      const pages = await Promise.all(
        chunks.map((chunk) =>
          rumApi.search({
            type: 'sessions',
            from: to - LOOKBACK_MICROS,
            to,
            filters: [{ field: 'session.id', op: 'in', value: JSON.stringify(chunk) }],
            columns: COLUMNS,
            limit: chunk.length,
          }),
        ),
      );

      const rows = new Map<string, RumRow>();

      for (const row of pages.flatMap((page) => page.items)) {
        const id = row['session.id'];

        if (typeof id === 'string') rows.set(id, row);
      }

      return rows;
    },
    enabled: Boolean(orgId && ids.length > 0),
    staleTime: 30_000,
  });
}

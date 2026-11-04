import { ChevronLeft, ChevronRight, ListVideo } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { cn } from '@/shell/lib/cn';

import { neighbours, sessionPath } from './moment';
import { usePlaylist } from '../playlists/usePlaylists';
import { useSessionRows } from '../playlists/useSessionRows';

const STEP =
  'inline-flex h-8 items-center gap-1 rounded-md bg-[var(--control-surface)] px-2.5 font-sans text-xs font-strong text-tx-1 outline-none transition-colors hover:bg-bg-3 focus-visible:bg-bg-3';

/**
 * Where the session stands in the playlist it is gone through in: the name of the playlist, which
 * session of how many this is, and the way to the one before and the one after.
 */
export function PlaylistBar({ playlistId, current }: { playlistId: string; current: string }) {
  const { t } = useTranslation('rum');
  const playlist = usePlaylist(playlistId);
  const sessions = React.useMemo(() => playlist.data?.sessions ?? [], [playlist.data]);
  const rows = useSessionRows(React.useMemo(() => sessions.map((entry) => entry.sessionId), [sessions]));

  if (!playlist.data) return null;

  const place = neighbours(sessions, current);
  const open = (entry: (typeof sessions)[number]) =>
    sessionPath(entry.sessionId, { session: rows.data?.get(entry.sessionId), at: entry.atTime, playlist: playlistId });

  return (
    <div
      data-rum-playlist-bar={playlistId}
      className="flex flex-wrap items-center gap-3 rounded-md bg-[var(--functional-surface)] px-3 py-2 [box-shadow:var(--shadow-functional-surface)]"
    >
      <ListVideo aria-hidden className="h-4 w-4 shrink-0 text-[var(--chart-4)]" />
      <Link
        to={`/rum/playlists/${encodeURIComponent(playlistId)}`}
        className="min-w-0 truncate font-sans text-sm font-strong text-tx-0 outline-none hover:underline focus-visible:underline"
      >
        {playlist.data.name}
      </Link>
      {place ? (
        <span data-rum-playlist-place="" className="shrink-0 font-sans text-xs tabular-nums text-tx-2">
          {t('playlists.bar.place', { index: place.index + 1, total: place.total })}
        </span>
      ) : (
        <span className="shrink-0 font-sans text-xs text-tx-3">{t('playlists.bar.not_in')}</span>
      )}
      {place && (
        <span className="ml-auto flex items-center gap-2">
          {place.previous ? (
            <Link to={open(place.previous)} data-rum-playlist-previous="" className={STEP}>
              <ChevronLeft aria-hidden className="h-3.5 w-3.5" />
              {t('playlists.bar.previous')}
            </Link>
          ) : (
            <span aria-disabled className={cn(STEP, 'pointer-events-none text-[var(--tx-disabled)]')}>
              <ChevronLeft aria-hidden className="h-3.5 w-3.5" />
              {t('playlists.bar.previous')}
            </span>
          )}
          {place.next ? (
            <Link to={open(place.next)} data-rum-playlist-next="" className={STEP}>
              {t('playlists.bar.next')}
              <ChevronRight aria-hidden className="h-3.5 w-3.5" />
            </Link>
          ) : (
            <span aria-disabled className={cn(STEP, 'pointer-events-none text-[var(--tx-disabled)]')}>
              {t('playlists.bar.next')}
              <ChevronRight aria-hidden className="h-3.5 w-3.5" />
            </span>
          )}
        </span>
      )}
    </div>
  );
}

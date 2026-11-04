import { useQuery } from '@tanstack/react-query';
import { Pencil, Play, Trash2, X } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link, useNavigate, useParams } from 'react-router-dom';

import { DataTable, type DataTableColumn } from '@/admin';
import { ConfirmDialog } from '@/admin';
import type { RumPlaylistSession } from '@/api/rum';
import * as rumApi from '@/api/rum';
import { toApiError } from '@/lib/http';
import { useTimeFormatter } from '@/lib/time';
import { useActionAccess } from '@/product/actionAccess';
import { productStateFor } from '@/product/states';
import { ChromeButton, IconButton } from '@/shell/chrome';
import { queryStateFor } from '@/shell/query/State';

import { formatDurationNs } from '../_helpers';
import { RumDetailPage, RumSectionHeader, RumSurface } from '../RumLayout';
import { PlaylistDrawer } from './PlaylistDrawer';
import { usePlaylist, usePlaylistChanges } from './usePlaylists';
import { useSessionRows } from './useSessionRows';
import { byName } from '../applications/model';
import { issueTotal, sessionHeadline, sessionIssues, sessionPage, sessionUser } from '../session/model';
import { sessionPath } from '../session/moment';

/** A moment as the player says it: minutes and seconds from the start of the session. */
function offset(at: number | null, start: number | undefined): string {
  if (at === null || start === undefined) return '—';

  const seconds = Math.max(0, Math.floor((at - start) / 1_000_000));

  return `${String(Math.floor(seconds / 60)).padStart(2, '0')}:${String(seconds % 60).padStart(2, '0')}`;
}

/**
 * One playlist: the sessions in it in the order they were added, each with what happened in it and
 * the moment it opens at. A session opens inside the playlist, which then offers the one before
 * and the one after.
 */
export function RumPlaylistPage() {
  const { id = '' } = useParams<{ id: string }>();

  return <PlaylistContent key={id} id={id} />;
}

function PlaylistContent({ id }: { id: string }) {
  const { t } = useTranslation('rum');
  const fmt = useTimeFormatter();
  const navigate = useNavigate();
  const manage = useActionAccess({ permission: 'rum.playlists.manage' });
  const changes = usePlaylistChanges();
  const [editing, setEditing] = React.useState(false);
  const [deleting, setDeleting] = React.useState(false);

  const playlist = usePlaylist(id);
  const detail = playlist.data;
  const entries = React.useMemo(() => detail?.sessions ?? [], [detail]);
  const rows = useSessionRows(React.useMemo(() => entries.map((entry) => entry.sessionId), [entries]));
  const sessions = rows.data;

  const applications = useQuery({ queryKey: ['rum', 'applications'], queryFn: () => rumApi.listApplications() });
  const applicationName = applications.data?.find((entry) => entry.id === detail?.application)?.name ?? detail?.application ?? '';

  const columns = React.useMemo<DataTableColumn<RumPlaylistSession>[]>(
    () => [
      { key: 'index', header: '#', width: 48, className: 'tabular-nums text-tx-3', cell: (entry) => entries.indexOf(entry) + 1 },
      {
        key: 'session',
        header: t('playlists.sessions.session'),
        cell: (entry) => {
          const row = sessions?.get(entry.sessionId);
          const headline = row ? sessionHeadline(row) : 'plain';

          return (
            <span className="block min-w-0">
              <span
                data-rum-playlist-session={entry.sessionId}
                className={headline === 'error' ? 'block truncate font-strong text-[var(--red-ink)]' : 'block truncate font-strong text-tx-0'}
              >
                {row ? (sessionPage(row) ?? '—') : t('playlists.sessions.gone')}
              </span>
              <span className="mt-0.5 block truncate font-mono text-xs text-tx-3">{entry.sessionId}</span>
            </span>
          );
        },
      },
      {
        key: 'person',
        header: t('playlists.sessions.person'),
        cell: (entry) => {
          const row = sessions?.get(entry.sessionId);

          return row ? (sessionUser(row) ?? '—') : '—';
        },
      },
      {
        key: 'started',
        header: t('playlists.sessions.started'),
        cell: (entry) => {
          const row = sessions?.get(entry.sessionId);

          return row ? <span className="tabular-nums text-tx-2">{fmt.micros(row.timestamp, false)}</span> : '—';
        },
      },
      {
        key: 'duration',
        header: t('playlists.sessions.duration'),
        className: 'tabular-nums',
        cell: (entry) => {
          const duration = sessions?.get(entry.sessionId)?.['session.duration'];

          return typeof duration === 'number' ? formatDurationNs(duration) : '—';
        },
      },
      {
        key: 'signs',
        header: t('playlists.sessions.signs'),
        cell: (entry) => {
          const row = sessions?.get(entry.sessionId);
          const issues = row ? sessionIssues(row) : null;

          if (!issues || issueTotal(issues) === 0) return <span className="text-tx-3">—</span>;

          return (
            <span className="flex flex-wrap gap-x-3">
              {issues.errors + issues.crashes > 0 && (
                <span className="text-[var(--red-ink)]">{t('playlists.sessions.errors', { count: issues.errors + issues.crashes })}</span>
              )}
              {issues.frustrations > 0 && (
                <span className="text-[var(--orange-ink)]">{t('playlists.sessions.frustrations', { count: issues.frustrations })}</span>
              )}
            </span>
          );
        },
      },
      {
        key: 'opens_at',
        header: t('playlists.sessions.opens_at'),
        className: 'tabular-nums',
        cell: (entry) => offset(entry.atTime, sessions?.get(entry.sessionId)?.timestamp),
      },
      {
        key: 'remove',
        header: <span className="sr-only">{t('playlists.sessions.remove_column')}</span>,
        width: 48,
        cell: (entry) => (
          <IconButton
            aria-label={t('playlists.sessions.remove', { id: entry.sessionId })}
            data-rum-playlist-remove={entry.sessionId}
            disabled={manage.disabled || changes.take.isPending}
            onClick={() => changes.take.mutate({ id, sessionId: entry.sessionId })}
          >
            <X aria-hidden className="h-3.5 w-3.5" />
          </IconButton>
        ),
      },
    ],
    [changes.take, entries, fmt, id, manage.disabled, sessions, t],
  );

  const first = entries[0];

  const toolbar = detail ? (
    <>
      {first && (
        <ChromeButton
          data-rum-playlist-play=""
          onClick={() => navigate(sessionPath(first.sessionId, { session: sessions?.get(first.sessionId), at: first.atTime, playlist: id }))}
        >
          <Play aria-hidden className="h-3.5 w-3.5" />
          {t('playlists.play')}
        </ChromeButton>
      )}
      <ChromeButton disabled={manage.disabled} disabledReason={manage.reason} onClick={() => setEditing(true)}>
        <Pencil aria-hidden className="h-3.5 w-3.5" />
        {t('playlists.rename')}
      </ChromeButton>
      <ChromeButton disabled={manage.disabled} disabledReason={manage.reason} onClick={() => setDeleting(true)}>
        <Trash2 aria-hidden className="h-3.5 w-3.5" />
        {t('playlists.delete')}
      </ChromeButton>
    </>
  ) : undefined;

  // A playlist that is not there is said so, not told as a failure of the page.
  const missing = playlist.isError && toApiError(playlist.error).status === 404;
  const state = productStateFor(
    queryStateFor({ isLoading: playlist.isLoading, isError: playlist.isError && !missing, data: detail }),
    { error: playlist.error, emptyTitle: t('playlists.not_found') },
  );

  return (
    <>
      <RumDetailPage
        title={detail?.name ?? t('playlists.title')}
        subtitle={detail ? applicationName : undefined}
        toolbar={toolbar}
        credit={false}
        state={state}
      >
        {detail && (
          <RumSurface data-rum-playlist={detail.id} className="space-y-4 p-4">
            <RumSectionHeader
              title={t('playlists.sessions.title')}
              description={detail.description || t('playlists.sessions.count', { count: entries.length })}
              action={
                <Link to="/rum/playlists" className="font-sans text-xs text-tx-2 outline-none hover:text-tx-0 focus-visible:text-tx-0">
                  {t('playlists.all')}
                </Link>
              }
            />
            {entries.length === 0 ? (
              <p className="m-0 py-6 text-center font-sans text-sm text-tx-3">{t('playlists.sessions.empty')}</p>
            ) : (
              <DataTable
                className="[&_tbody_tr]:h-auto [&_tbody_td]:py-2"
                rows={entries}
                columns={columns}
                rowKey={(entry) => entry.sessionId}
                onRowClick={(entry) =>
                  navigate(sessionPath(entry.sessionId, { session: sessions?.get(entry.sessionId), at: entry.atTime, playlist: id }))
                }
              />
            )}
          </RumSurface>
        )}
      </RumDetailPage>
      {detail && (
        <PlaylistDrawer open={editing} onOpenChange={setEditing} playlist={detail} applications={byName(applications.data ?? [])} />
      )}
      <ConfirmDialog
        open={deleting}
        onOpenChange={setDeleting}
        title={t('playlists.delete_confirm_title', { name: detail?.name ?? '' })}
        description={t('playlists.delete_confirm_description')}
        confirmLabel={t('playlists.delete')}
        cancelLabel={t('playlists.cancel')}
        destructive
        busy={changes.remove.isPending}
        onConfirm={() =>
          changes.remove.mutate(id, {
            onSuccess: () => {
              setDeleting(false);
              navigate('/rum/playlists', { replace: true });
            },
          })
        }
      />
    </>
  );
}

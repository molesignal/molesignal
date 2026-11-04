import { useQuery } from '@tanstack/react-query';
import { ListVideo, Plus } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link, useNavigate, useSearchParams } from 'react-router-dom';

import { DataTable, type DataTableColumn } from '@/admin';
import * as rumApi from '@/api/rum';
import type { RumApplication, RumPlaylist } from '@/api/rum';
import { useTimeFormatter } from '@/lib/time';
import { useActionAccess } from '@/product/actionAccess';
import { productStateFor } from '@/product/states';
import { ChromeButton } from '@/shell/chrome';
import { queryStateFor } from '@/shell/query/State';
import { FittedSelect } from '@/shell/ui/fitted-select';
import { useUsers } from '@/shell/useUsers';

import { byName } from '../applications/model';
import { CONTROL } from '../overview/ScopeRow';
import { RumListPage, RumSectionHeader, RumSurface } from '../RumLayout';
import { PlaylistDrawer } from './PlaylistDrawer';
import { usePlaylists } from './usePlaylists';

const ALL = '__all__';

/**
 * The playlists of sessions a team keeps to go through one after the other, of every application or
 * of one. A playlist is made here or from the session page, and a session is put in from there.
 */
export function RumPlaylists() {
  const { t } = useTranslation('rum');
  const fmt = useTimeFormatter();
  const navigate = useNavigate();
  const { nameOf } = useUsers();
  const manage = useActionAccess({ permission: 'rum.playlists.manage' });
  const [params, setParams] = useSearchParams();
  const [creating, setCreating] = React.useState(false);

  const applicationsQuery = useQuery({ queryKey: ['rum', 'applications'], queryFn: () => rumApi.listApplications() });
  const applications = React.useMemo(() => byName(applicationsQuery.data ?? []), [applicationsQuery.data]);
  const names = React.useMemo(() => new Map(applications.map((application) => [application.id, application.name])), [applications]);

  const chosen = params.get('app') ?? '';
  const application = applications.some((entry) => entry.id === chosen) ? chosen : '';
  const query = usePlaylists(application || undefined);
  const playlists = query.data ?? [];

  const choose = (value: string) =>
    setParams(
      (current) => {
        const next = new URLSearchParams(current);

        if (value === ALL) next.delete('app');
        else next.set('app', value);

        return next;
      },
      { replace: true },
    );

  const createButton = (
    <ChromeButton
      variant="primary"
      data-rum-playlist-create=""
      disabled={manage.disabled || applications.length === 0}
      disabledReason={manage.reason}
      onClick={() => setCreating(true)}
    >
      <Plus aria-hidden className="h-3.5 w-3.5" />
      {t('playlists.create')}
    </ChromeButton>
  );

  const columns = React.useMemo<DataTableColumn<RumPlaylist>[]>(
    () => [
      {
        key: 'name',
        header: t('playlists.columns.name'),
        cell: (row) => (
          <span className="block min-w-0">
            <Link
              to={`/rum/playlists/${encodeURIComponent(row.id)}`}
              data-rum-playlist-link={row.id}
              className="block truncate font-strong text-tx-0 outline-none hover:underline focus-visible:underline"
            >
              {row.name}
            </Link>
            {row.description && <span className="mt-0.5 block truncate text-xs text-tx-3">{row.description}</span>}
          </span>
        ),
      },
      { key: 'application', header: t('playlists.columns.application'), cell: (row) => names.get(row.application) ?? row.application },
      {
        key: 'sessions',
        header: t('playlists.columns.sessions'),
        className: 'tabular-nums',
        cell: (row) => <span data-rum-playlist-sessions={row.sessionCount}>{row.sessionCount}</span>,
      },
      {
        key: 'updated',
        header: t('playlists.columns.updated'),
        cell: (row) => <span className="tabular-nums text-tx-2">{fmt.micros(row.updatedAt, false)}</span>,
      },
      {
        key: 'created_by',
        header: t('playlists.columns.created_by'),
        cell: (row) => (row.createdBy ? nameOf(row.createdBy) : '—'),
      },
    ],
    [fmt, nameOf, names, t],
  );

  const state = productStateFor(
    queryStateFor({ isLoading: query.isLoading || applicationsQuery.isLoading, isError: query.isError, data: playlists }),
    {
      error: query.error,
      emptyTitle: t('playlists.empty_title'),
      emptyDescription: applications.length === 0 ? t('playlists.empty_no_application') : t('playlists.empty_description'),
      emptyAction: applications.length === 0 ? undefined : createButton,
    },
  );

  return (
    <>
      <RumListPage toolbar={playlists.length > 0 ? createButton : undefined} state={state}>
        <RumSurface data-rum-playlists="" className="space-y-4 p-4">
          <RumSectionHeader
            title={
              <span className="inline-flex items-center gap-1.5">
                <ListVideo aria-hidden className="h-4 w-4 text-[var(--chart-4)]" />
                {t('playlists.title')}
              </span>
            }
            description={t('playlists.result_count', { count: playlists.length })}
            action={
              applications.length > 1 ? (
                <FittedSelect
                  aria-label={t('playlists.application_filter')}
                  data-rum-playlist-application=""
                  value={application || ALL}
                  onValueChange={choose}
                  options={[
                    { value: ALL, label: t('playlists.all_applications') },
                    ...applications.map((entry: RumApplication) => ({ value: entry.id, label: entry.name })),
                  ]}
                  maxWidth={260}
                  className={CONTROL}
                />
              ) : undefined
            }
          />
          <DataTable
            className="[&_tbody_tr]:h-auto [&_tbody_td]:py-2"
            rows={playlists}
            columns={columns}
            rowKey={(row) => row.id}
            onRowClick={(row) => navigate(`/rum/playlists/${encodeURIComponent(row.id)}`)}
          />
        </RumSurface>
      </RumListPage>
      <PlaylistDrawer
        open={creating}
        onOpenChange={setCreating}
        applications={applications}
        application={application}
        onSaved={(id) => navigate(`/rum/playlists/${encodeURIComponent(id)}`)}
      />
    </>
  );
}

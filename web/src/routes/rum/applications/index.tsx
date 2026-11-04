import { useQuery } from '@tanstack/react-query';
import { Plus } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useSearchParams } from 'react-router-dom';

import { DataTable, type DataTableColumn } from '@/admin';
import * as rumApi from '@/api/rum';
import type { RumApplication } from '@/api/rum';
import { useTimeFormatter } from '@/lib/time';
import { useActionAccess } from '@/product/actionAccess';
import { productStateFor } from '@/product/states';
import { ChromeButton } from '@/shell/chrome';
import { queryStateFor } from '@/shell/query/State';

import { RumListPage, RumSectionHeader, RumSurface } from '../RumLayout';
import { CreateApplicationDrawer } from './CreateDrawer';
import { ApplicationDrawer } from './Drawer';
import { byName } from './model';

/**
 * The applications that send RUM data. An application is created before any SDK sends to it:
 * its client token is bound to it, and an event for any other is refused.
 */
export function Applications() {
  const { t } = useTranslation('rum');
  const fmt = useTimeFormatter();
  const create = useActionAccess({ permission: 'rum.applications.manage' });
  const [searchParams, setSearchParams] = useSearchParams();
  // The explorer sends a visitor who has no application yet here to make one.
  const [creating, setCreating] = React.useState(() => searchParams.get('create') === '1');
  const [openId, setOpenId] = React.useState<string | null>(null);

  // The request is taken in once: a reload of the page does not open the form again.
  React.useEffect(() => {
    if (!searchParams.has('create')) return;

    setSearchParams(
      (current) => {
        const next = new URLSearchParams(current);
        next.delete('create');
        return next;
      },
      { replace: true },
    );
  }, [searchParams, setSearchParams]);

  const query = useQuery({
    queryKey: ['rum', 'applications'],
    queryFn: () => rumApi.listApplications(),
  });
  const applications = React.useMemo(() => byName(query.data ?? []), [query.data]);
  const opened = applications.find((application) => application.id === openId) ?? null;

  const createButton = (
    <ChromeButton
      variant="primary"
      disabled={create.disabled}
      disabledReason={create.reason}
      onClick={() => setCreating(true)}
    >
      <Plus aria-hidden className="h-3.5 w-3.5" />
      {t('applications.create_button')}
    </ChromeButton>
  );

  const columns = React.useMemo<DataTableColumn<RumApplication>[]>(
    () => [
      {
        key: 'application',
        header: t('applications.columns.application'),
        cell: (row) => (
          <span className="block min-w-0">
            <span className="block truncate font-strong text-tx-0">{row.name}</span>
            <span className="mt-0.5 block truncate text-xs text-tx-3">{row.id}</span>
          </span>
        ),
      },
      {
        key: 'platform',
        header: t('applications.columns.platform'),
        cell: (row) => t(`applications.platforms.${row.platform}`),
      },
      {
        key: 'collection',
        header: t('applications.columns.collection'),
        cell: (row) => (
          <span className="flex flex-wrap gap-x-3">
            <span className={row.settings.collectIp ? 'text-tx-1' : 'text-tx-3 line-through'}>
              {t('applications.collection.ip')}
            </span>
            <span className={row.settings.collectGeo ? 'text-tx-1' : 'text-tx-3 line-through'}>
              {t('applications.collection.geo')}
            </span>
          </span>
        ),
      },
      {
        key: 'created',
        header: t('applications.columns.created'),
        cell: (row) => <span className="tabular-nums text-tx-2">{fmt.micros(row.createdAt, false)}</span>,
      },
    ],
    [fmt, t],
  );

  const state = productStateFor(
    queryStateFor({ isLoading: query.isLoading, isError: query.isError, data: applications }),
    {
      error: query.error,
      emptyTitle: t('applications.empty_title'),
      emptyDescription: t('applications.empty_description'),
      emptyAction: createButton,
    },
  );

  return (
    <>
      <RumListPage toolbar={applications.length > 0 ? createButton : undefined} state={state}>
        <RumSurface className="space-y-4 p-4">
          <RumSectionHeader
            title={t('applications.list_title')}
            description={t('applications.result_count', { count: applications.length })}
          />
          <DataTable
            className="[&_tbody_tr]:h-auto [&_tbody_td]:py-2"
            rows={applications}
            columns={columns}
            rowKey={(row) => row.id}
            onRowClick={(row) => setOpenId(row.id)}
          />
        </RumSurface>
      </RumListPage>
      <CreateApplicationDrawer
        open={creating}
        onOpenChange={setCreating}
        onCreated={(application) => setOpenId(application.id)}
      />
      <ApplicationDrawer application={opened} onClose={() => setOpenId(null)} />
    </>
  );
}

import { useQuery } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { DataTable } from '@/admin';
import * as notifyApi from '@/api/notify';
import { productStateFor } from '@/product/states';
import { Pill } from '@/shell/chrome';
import { FormDrawer } from '@/shell/FormDrawer';
import { queryStateFor } from '@/shell/query/State';

import { connectorName, primaryEndpoint } from './model';
import { NotifySettingsPage } from './SettingsPage';
import { UserNotifyPanel } from './UserNotifyPanel';

export function NotifyUsersPage() {
  const { t } = useTranslation('notify');
  const [selected, setSelected] = React.useState<notifyApi.NotifyUserSummary | null>(null);
  const users = useQuery({
    queryKey: ['notify', 'users'],
    queryFn: notifyApi.listNotifyUsers,
  });
  const connectors = useQuery({
    queryKey: ['notify', 'connectors'],
    queryFn: notifyApi.listConnectors,
  });
  const rows = users.data ?? [];
  const state = productStateFor(
    queryStateFor({
      isLoading: users.isLoading,
      isError: users.isError,
      data: rows,
    }),
    {
      error: users.error,
      emptyTitle: t('users.empty_title'),
      emptyDescription: t('users.empty_description'),
    },
  );
  const connectorRows = connectors.data ?? [];
  const primary = (
    row: notifyApi.NotifyUserSummary,
    category: notifyApi.NotifyCategory,
  ) => {
    const endpoint = primaryEndpoint(
      row.preferences.find((value) => value.category === category),
      row.endpoints,
    );
    return endpoint ? connectorName(connectorRows, endpoint.connector_id) : '—';
  };

  return (
    <>
      <NotifySettingsPage
        title={t('users.title')}
        subtitle={t('users.subtitle')}
        state={state}
      >
        <div className="overflow-x-auto rounded-md border border-bd-0 bg-bg-1">
          <DataTable
            rows={rows}
            rowKey={(row) => row.user_id}
            onRowClick={setSelected}
            columns={[
              {
                key: 'user',
                header: t('users.columns.user'),
                width: '28%',
                cell: (row) => (
                  <div className="flex min-w-0 items-center gap-3">
                    <span className="grid h-8 w-8 shrink-0 place-items-center rounded-full bg-indigo-dim text-xs font-bold text-indigo-soft">
                      {(row.display_name || row.email).slice(0, 1).toUpperCase()}
                    </span>
                    <div className="min-w-0">
                      <div className="truncate text-sm font-semibold text-tx-0">
                        {row.display_name || row.email}
                      </div>
                      <div className="truncate text-xs text-tx-3">{row.email}</div>
                    </div>
                  </div>
                ),
              },
              {
                key: 'endpoints',
                header: t('users.columns.endpoints'),
                width: '22%',
                cell: (row) => (
                  <div className="flex flex-wrap gap-1">
                    {row.endpoints.length === 0
                      ? '—'
                      : Array.from(
                          new Set(
                            row.endpoints.map((endpoint) =>
                              connectorName(connectorRows, endpoint.connector_id),
                            ),
                          ),
                        ).map((name) => <Pill key={name} tone="dim">{name}</Pill>)}
                  </div>
                ),
              },
              {
                key: 'alert',
                header: t('users.columns.alert'),
                width: 140,
                cell: (row) => <span className="text-xs text-tx-2">{primary(row, 'alert')}</span>,
              },
              {
                key: 'oncall',
                header: t('users.columns.oncall'),
                width: 140,
                cell: (row) => <span className="text-xs text-tx-2">{primary(row, 'oncall')}</span>,
              },
              {
                key: 'report',
                header: t('users.columns.report'),
                width: 140,
                cell: (row) => <span className="text-xs text-tx-2">{primary(row, 'report')}</span>,
              },
              {
                key: 'status',
                header: t('users.columns.status'),
                width: 110,
                cell: (row) => {
                  const configured = row.endpoints.length > 0 && row.preferences.length > 0;
                  return (
                    <Pill tone={configured ? 'green' : 'dim'}>
                      {t(configured ? 'common.configured' : 'common.not_configured')}
                    </Pill>
                  );
                },
              },
            ]}
          />
        </div>
      </NotifySettingsPage>
      <FormDrawer
        open={selected !== null}
        onOpenChange={(open) => !open && setSelected(null)}
        width={980}
        title={t('users.drawer_title', {
          name: selected?.display_name || selected?.email || '',
        })}
        subtitle={t('users.drawer_subtitle')}
        footer={null}
      >
        {selected && <UserNotifyPanel userId={selected.user_id} />}
      </FormDrawer>
    </>
  );
}

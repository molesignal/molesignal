import { useQuery } from '@tanstack/react-query';
import { Plus } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { DataTable } from '@/admin';
import * as serviceAccountsApi from '@/api/serviceAccounts';
import { useActionAccess } from '@/product/actionAccess';
import { productStateFor } from '@/product/states';
import { ChromeButton, Pill } from '@/shell/chrome';
import { queryStateFor } from '@/shell/query/State';

import { IamListPage } from '../IamLayout';
import { CreateServiceAccountDrawer } from './CreateServiceAccountDrawer';
import { ManageServiceAccountDrawer } from './ManageServiceAccountDrawer';

export function ServiceAccounts() {
  const { t } = useTranslation('iam');
  const manageAccess = useActionAccess({
    permission: 'service_accounts.manage',
  });
  const [creating, setCreating] = React.useState(false);
  const [selected, setSelected] =
    React.useState<serviceAccountsApi.ServiceAccount | null>(null);
  const query = useQuery({
    queryKey: ['iam', 'service-accounts'],
    queryFn: serviceAccountsApi.list,
  });
  const rows = query.data ?? [];
  const queryState = queryStateFor({
    isLoading: query.isLoading,
    isError: query.isError,
    data: rows,
  });

  return (
    <>
      <IamListPage
        title={t('service_accounts.title')}
        subtitle={t('service_accounts.subtitle')}
        toolbar={
          <ChromeButton
            variant="primary"
            onClick={() => setCreating(true)}
            disabled={manageAccess.disabled}
            disabledReason={manageAccess.reason}
          >
            <Plus className="h-3 w-3" />
            {t('service_accounts.create')}
          </ChromeButton>
        }
        state={productStateFor(
          queryState === 'empty' ? null : queryState,
          { error: query.error },
        )}
      >
        <DataTable
          rows={rows}
          rowKey={(account) => account.id}
          onRowClick={setSelected}
          emptyLabel={t('service_accounts.empty_title')}
          columns={[
            {
              key: 'name',
              header: t('service_accounts.columns.name'),
              cell: (account) => (
                <div className="min-w-0">
                  <div className="truncate text-sm text-tx-0">{account.name}</div>
                  <div className="truncate text-xs text-tx-3">{account.id}</div>
                </div>
              ),
              width: 260,
            },
            {
              key: 'description',
              header: t('service_accounts.columns.description'),
              cell: (account) => account.description || '—',
            },
            {
              key: 'role',
              header: t('service_accounts.columns.role'),
              cell: (account) => account.role_name,
              width: 160,
            },
            {
              key: 'status',
              header: t('service_accounts.columns.status'),
              cell: (account) => (
                <Pill tone={account.disabled ? 'dim' : 'green'}>
                  {t(
                    `service_accounts.status_${account.disabled ? 'disabled' : 'active'}`,
                  )}
                </Pill>
              ),
              width: 120,
            },
          ]}
        />
      </IamListPage>
      <CreateServiceAccountDrawer
        open={creating}
        access={manageAccess}
        onClose={() => setCreating(false)}
      />
      <ManageServiceAccountDrawer
        account={selected}
        access={manageAccess}
        onClose={() => setSelected(null)}
      />
    </>
  );
}

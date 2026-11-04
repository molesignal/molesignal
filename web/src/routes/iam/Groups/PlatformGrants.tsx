import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Plus, Trash2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog, DataTable } from '@/admin';
import * as grantsApi from '@/api/groups';
import * as orgsApi from '@/api/orgs';
import * as rolesApi from '@/api/roles';
import { toApiError } from '@/lib/http';
import { restrictActionAccess, useActionAccess } from '@/product/actionAccess';
import { productStateFor } from '@/product/states';
import { ChromeButton, IconButton } from '@/shell/chrome';
import { queryStateFor } from '@/shell/query/State';
import { toast } from '@/shell/ui/sonner';

import { PlatformGrantDrawer } from './PlatformGrantDrawer';
import { invalidateIamAccess } from './shared';
import { formatMicros } from '../../rum/_helpers';
import { IamListPage } from '../IamLayout';

/**
 * Who holds what on the platform. The platform organization has no members: a grant is a role bound
 * to an account, across the platform or on one organization.
 */
export function PlatformGrants() {
  const { t } = useTranslation('iam');
  const qc = useQueryClient();
  const [creating, setCreating] = React.useState(false);
  const [removing, setRemoving] = React.useState<grantsApi.RoleBinding | null>(null);
  const manageAccess = useActionAccess({ permission: 'sys.administrators.manage' });

  const bindingsQuery = useQuery({
    queryKey: ['iam', 'role-bindings', 'platform'],
    queryFn: () => grantsApi.listRoleBindings(),
  });
  const rolesQuery = useQuery({ queryKey: ['iam', 'roles'], queryFn: () => rolesApi.list() });
  const orgsQuery = useQuery({ queryKey: ['iam', 'orgs'], queryFn: () => orgsApi.listOrgs() });
  const bindings = bindingsQuery.data ?? [];
  const accountIds = [...new Set(bindings.map((binding) => binding.principalId))].sort();
  const accountsQuery = useQuery({
    queryKey: ['iam', 'platform-accounts', accountIds],
    queryFn: () => grantsApi.platformAccountsByIds(accountIds),
    enabled: accountIds.length > 0,
  });

  const remove = useMutation({
    mutationFn: (binding: grantsApi.RoleBinding) => grantsApi.removeRoleBinding(binding.id),
    onSuccess: async () => {
      setRemoving(null);
      toast.success(t('groups.toast_deleted'));
      await invalidateIamAccess(qc);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const newGrant = (
    <ChromeButton
      variant="primary"
      disabled={manageAccess.disabled}
      disabledReason={manageAccess.reason}
      onClick={() => manageAccess.allowed && setCreating(true)}
    >
      <Plus className="h-3 w-3" />
      {t('groups.new_grant')}
    </ChromeButton>
  );
  const pageState = productStateFor(
    queryStateFor({
      isLoading: bindingsQuery.isLoading,
      isError: bindingsQuery.isError,
      data: bindings,
    }),
    {
      error: bindingsQuery.error,
      emptyTitle: t('groups.platform.empty_title'),
      emptyDescription: t('groups.platform.empty_description'),
      emptyAction: newGrant,
    },
  );

  const rolesById = new Map((rolesQuery.data ?? []).map((role) => [role.id, role]));
  const orgsById = new Map((orgsQuery.data ?? []).map((org) => [org.id, org]));
  const accountsById = new Map((accountsQuery.data ?? []).map((account) => [account.id, account]));

  return (
    <>
      <IamListPage
        title={t('groups.platform.title')}
        subtitle={t('groups.platform.subtitle') as string}
        toolbar={newGrant}
        state={pageState}
      >
        <DataTable
          rows={bindings}
          rowKey={(row) => row.id}
          emptyLabel={t('groups.platform.empty_title')}
          columns={[
            {
              key: 'account',
              header: t('groups.platform.columns.account'),
              cell: (row) => {
                const account = accountsById.get(row.principalId);

                return (
                  <div className="min-w-0">
                    <div className="truncate text-tx-0">
                      {account?.displayName || t('groups.unknown_principal')}
                    </div>
                    <div className="truncate text-xs text-tx-3">{account?.email ?? row.principalId}</div>
                  </div>
                );
              },
            },
            {
              key: 'role',
              header: t('groups.columns.role'),
              cell: (row) => rolesById.get(row.roleId)?.name ?? t('groups.unknown_role'),
            },
            {
              key: 'scope',
              header: t('groups.platform.columns.scope'),
              cell: (row) =>
                row.resourceId
                  ? (orgsById.get(row.resourceId)?.name ?? row.resourceId)
                  : t('groups.platform.whole_platform'),
            },
            {
              key: 'validity',
              header: t('groups.columns.validity'),
              cell: (row) =>
                row.expiresAt ? formatMicros(row.expiresAt) : t('groups.no_expiry'),
            },
            {
              key: 'actions',
              header: '',
              width: 60,
              cell: (row) => {
                const deleteAccess = restrictActionAccess(
                  manageAccess,
                  !remove.isPending,
                  t('groups.action_pending'),
                );

                return (
                  <IconButton
                    disabled={deleteAccess.disabled}
                    disabledReason={deleteAccess.reason}
                    onClick={(event) => {
                      event.stopPropagation();
                      if (deleteAccess.allowed) setRemoving(row);
                    }}
                    aria-label={t('groups.delete_grant')}
                    className="enabled:hover:bg-red-dim enabled:hover:text-red-soft"
                  >
                    <Trash2 className="h-3.5 w-3.5" />
                  </IconButton>
                );
              },
            },
          ]}
        />
      </IamListPage>

      <PlatformGrantDrawer
        access={manageAccess}
        organizations={orgsQuery.data ?? []}
        roles={rolesQuery.data ?? []}
        open={creating}
        onClose={() => setCreating(false)}
      />
      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(open) => !open && setRemoving(null)}
        destructive
        title={t('groups.delete_confirm_title')}
        description={t('groups.platform.delete_confirm_description')}
        confirmLabel={t('groups.delete')}
        busy={remove.isPending}
        disabled={manageAccess.disabled}
        disabledReason={manageAccess.reason}
        onConfirm={() => {
          if (manageAccess.allowed && removing) remove.mutate(removing);
        }}
      />
    </>
  );
}

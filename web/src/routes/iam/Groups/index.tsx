import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Check, Plus, Share2, Trash2, X } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog, DataTable } from '@/admin';
import * as grantsApi from '@/api/groups';
import * as rolesApi from '@/api/roles';
import * as teamsApi from '@/api/teams';
import * as usersApi from '@/api/users';
import { toApiError } from '@/lib/http';
import {
  restrictActionAccess,
  useActionAccess,
} from '@/product/actionAccess';
import { useIamPermissionCatalog } from '@/product/iamCatalog';
import { productStateFor } from '@/product/states';
import { ChromeButton, IconButton, Pill } from '@/shell/chrome';
import { queryStateFor } from '@/shell/query/State';
import { toast } from '@/shell/ui/sonner';
import { useCurrentOrgSelection } from '@/stores/useOrgStore';

import { AccessGrantDrawer } from './AccessGrantDrawer';
import { CrossOrgGrantDrawer } from './CrossOrgGrantDrawer';
import {
  invalidateIamAccess,
  PermissionPills,
  principalLabel,
  SectionTitle,
} from './shared';
import { formatMicros } from '../../rum/_helpers';
import { IamListPage } from '../IamLayout';

export function Groups() {
  const { t } = useTranslation('iam');
  const qc = useQueryClient();
  const [creating, setCreating] = React.useState(false);
  const [sharing, setSharing] = React.useState(false);
  const [removing, setRemoving] =
    React.useState<grantsApi.RoleBinding | null>(null);
  const { currentOrgId } = useCurrentOrgSelection();
  const manageAccess = useActionAccess({
    permission: 'iam.policies.manage',
  });
  const catalogQuery = useIamPermissionCatalog();

  const bindingsQuery = useQuery({
    queryKey: ['iam', 'role-bindings', currentOrgId],
    queryFn: () => grantsApi.listRoleBindings(),
  });
  const grantsQuery = useQuery({
    queryKey: ['iam', 'cross-org-grants', currentOrgId],
    queryFn: () => grantsApi.listCrossOrgGrants(),
  });
  const rolesQuery = useQuery({
    queryKey: ['iam', 'roles'],
    queryFn: () => rolesApi.list(),
  });
  const usersQuery = useQuery({
    queryKey: ['iam', 'grant-users'],
    queryFn: () => usersApi.list(),
  });
  const teamsQuery = useQuery({
    queryKey: ['iam', 'grant-teams'],
    queryFn: () => teamsApi.list(),
  });

  const bindings = bindingsQuery.data ?? [];
  const grants = grantsQuery.data ?? [];
  const pageState = productStateFor(
    queryStateFor({
      isLoading:
        bindingsQuery.isLoading ||
        grantsQuery.isLoading ||
        catalogQuery.isLoading,
      isError:
        bindingsQuery.isError ||
        grantsQuery.isError ||
        catalogQuery.isError,
      data: [...bindings, ...grants],
    }),
    {
      error: bindingsQuery.error ?? grantsQuery.error ?? catalogQuery.error,
      emptyTitle: t('groups.empty_title'),
      emptyDescription: t('groups.empty_description'),
      emptyAction: (
        <ChromeButton
          variant="primary"
          disabled={manageAccess.disabled}
          disabledReason={manageAccess.reason}
          onClick={() => manageAccess.allowed && setCreating(true)}
        >
          <Plus className="h-3.5 w-3.5" />
          {t('groups.new_grant')}
        </ChromeButton>
      ),
    },
  );

  const remove = useMutation({
    mutationFn: (binding: grantsApi.RoleBinding) =>
      grantsApi.removeRoleBinding(binding.id),
    onSuccess: async () => {
      setRemoving(null);
      toast.success(t('groups.toast_deleted'));
      await invalidateIamAccess(qc);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const changeGrantStatus = useMutation({
    mutationFn: ({
      grant,
      status,
    }: {
      grant: grantsApi.CrossOrgGrant;
      status: 'active' | 'revoked';
    }) =>
      status === 'active'
        ? grantsApi.acceptCrossOrgGrant(grant.id)
        : grantsApi.revokeCrossOrgGrant(grant.id),
    onSuccess: async (_response, request) => {
      toast.success(
        request.status === 'active'
          ? t('groups.toast_accepted')
          : t('groups.toast_revoked'),
      );
      await invalidateIamAccess(qc);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const rolesById = new Map(
    (rolesQuery.data ?? []).map((role) => [role.id, role]),
  );
  const usersById = new Map(
    (usersQuery.data ?? []).map((user) => [user.id, user]),
  );
  const teamsById = new Map(
    (teamsQuery.data ?? []).map((team) => [team.id, team]),
  );

  return (
    <>
      <IamListPage
        title={t('groups.title')}
        subtitle={t('groups.subtitle') as string}
        toolbar={
          <div className="flex items-center gap-2">
            <ChromeButton
              disabled={manageAccess.disabled}
              disabledReason={manageAccess.reason}
              onClick={() => manageAccess.allowed && setSharing(true)}
            >
              <Share2 className="h-3 w-3" />
              {t('groups.share_cross_org')}
            </ChromeButton>
            <ChromeButton
              variant="primary"
              disabled={manageAccess.disabled}
              disabledReason={manageAccess.reason}
              onClick={() => manageAccess.allowed && setCreating(true)}
            >
              <Plus className="h-3 w-3" />
              {t('groups.new_grant')}
            </ChromeButton>
          </div>
        }
        state={pageState}
      >
        <div className="space-y-6">
          <section>
            <SectionTitle
              title={t('groups.sections.access_grants')}
              description={t('groups.sections.access_grants_hint')}
            />
            <DataTable
              rows={bindings}
              rowKey={(row) => row.id}
              emptyLabel={t('groups.empty_title')}
              columns={[
                {
                  key: 'principal',
                  header: t('groups.columns.principal'),
                  cell: (row) =>
                    principalLabel(row, usersById, teamsById, t),
                },
                {
                  key: 'role',
                  header: t('groups.columns.role'),
                  cell: (row) => {
                    const role = rolesById.get(row.role_id);
                    return role ? (
                      <div>
                        <div className="text-tx-0">{role.name}</div>
                        <div className="text-xs text-tx-3">
                          {role.permissions.length}{' '}
                          {t('groups.permissions_count')}
                        </div>
                      </div>
                    ) : (
                      t('groups.unknown_role')
                    );
                  },
                },
                {
                  key: 'resource',
                  header: t('groups.columns.resource'),
                  cell: (row) =>
                    row.resource_type
                      ? `${t(`groups.resources.${row.resource_type}`)} · ${
                          row.resource_id ?? t('groups.all_resources')
                        }`
                      : t('groups.organization_wide'),
                },
                {
                  key: 'validity',
                  header: t('groups.columns.validity'),
                  cell: (row) =>
                    row.expires_at
                      ? formatMicros(row.expires_at)
                      : t('groups.no_expiry'),
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
          </section>

          <section>
            <SectionTitle
              title={t('groups.sections.cross_org')}
              description={t('groups.sections.cross_org_hint')}
            />
            <DataTable
              rows={grants}
              rowKey={(row) => row.id}
              emptyLabel={t('groups.no_cross_org_grants')}
              columns={[
                {
                  key: 'organizations',
                  header: t('groups.columns.organizations'),
                  cell: (row) =>
                    `${row.source_organization_id} → ${row.target_organization_id}`,
                },
                {
                  key: 'resource',
                  header: t('groups.columns.resource'),
                  cell: (row) =>
                    `${t(`groups.resources.${row.resource_type}`)} · ${
                      row.resource_selector.ids?.join(', ') ??
                      t('groups.all_resources')
                    }`,
                },
                {
                  key: 'permissions',
                  header: t('groups.columns.permissions'),
                  cell: (row) => (
                    <PermissionPills
                      catalog={catalogQuery.data}
                      permissions={row.permissions}
                    />
                  ),
                },
                {
                  key: 'status',
                  header: t('groups.columns.status'),
                  width: 110,
                  cell: (row) => (
                    <Pill
                      tone={
                        row.status === 'active'
                          ? 'green'
                          : row.status === 'revoked'
                            ? 'red'
                            : 'orange'
                      }
                    >
                      {t(`groups.status.${row.status}`)}
                    </Pill>
                  ),
                },
                {
                  key: 'expiry',
                  header: t('groups.columns.validity'),
                  cell: (row) =>
                    row.expires_at
                      ? formatMicros(row.expires_at)
                      : t('groups.no_expiry'),
                },
                {
                  key: 'actions',
                  header: t('groups.columns.actions'),
                  width: 170,
                  cell: (row) => {
                    const acceptAccess = restrictActionAccess(
                      manageAccess,
                      row.status === 'pending' &&
                        row.target_organization_id === currentOrgId &&
                        !changeGrantStatus.isPending,
                      row.status !== 'pending'
                        ? t('groups.accept_pending_only')
                        : row.target_organization_id !== currentOrgId
                          ? t('groups.accept_target_only')
                          : t('groups.action_pending'),
                    );
                    const revokeAccess = restrictActionAccess(
                      manageAccess,
                      row.status !== 'revoked' && !changeGrantStatus.isPending,
                      row.status === 'revoked'
                        ? t('groups.already_revoked')
                        : t('groups.action_pending'),
                    );
                    return (
                      <div className="flex items-center gap-1">
                        <ChromeButton
                          size="sm"
                          variant="ghost"
                          disabled={acceptAccess.disabled}
                          disabledReason={acceptAccess.reason}
                          className="text-green-soft enabled:hover:bg-green-dim"
                          onClick={() => {
                            if (acceptAccess.allowed) {
                              changeGrantStatus.mutate({
                                grant: row,
                                status: 'active',
                              });
                            }
                          }}
                        >
                          <Check className="h-3.5 w-3.5" />
                          {t('groups.accept')}
                        </ChromeButton>
                        <ChromeButton
                          size="sm"
                          variant="ghost"
                          disabled={revokeAccess.disabled}
                          disabledReason={revokeAccess.reason}
                          className="text-red-soft enabled:hover:bg-red-dim"
                          onClick={() => {
                            if (revokeAccess.allowed) {
                              changeGrantStatus.mutate({
                                grant: row,
                                status: 'revoked',
                              });
                            }
                          }}
                        >
                          <X className="h-3.5 w-3.5" />
                          {t('groups.revoke')}
                        </ChromeButton>
                      </div>
                    );
                  },
                },
              ]}
            />
          </section>
        </div>
      </IamListPage>

      <AccessGrantDrawer
        access={manageAccess}
        catalog={catalogQuery.data}
        open={creating}
        onClose={() => setCreating(false)}
      />
      <CrossOrgGrantDrawer
        access={manageAccess}
        catalog={catalogQuery.data}
        open={sharing}
        onClose={() => setSharing(false)}
      />
      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(open) => !open && setRemoving(null)}
        destructive
        title={t('groups.delete_confirm_title')}
        description={t('groups.delete_confirm_description')}
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

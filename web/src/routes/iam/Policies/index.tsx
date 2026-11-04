import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Copy, Eye, Pencil, Plus, Trash2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog, DataTable } from '@/admin';
import * as policiesApi from '@/api/policies';
import { toApiError } from '@/lib/http';
import { useSessionPermissionScope } from '@/product/access';
import { useActionAccess } from '@/product/actionAccess';
import { useIamPermissionCatalog } from '@/product/iamCatalog';
import { productStateFor } from '@/product/states';
import { ChromeButton, IconButton } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { queryStateFor } from '@/shell/query/State';
import { toast } from '@/shell/ui/sonner';

import { IamListPage } from '../IamLayout';
import { PermissionSummary } from '../PermissionSummary';
import { policyDescription, policyName } from './labels';
import { type PolicyDrawerMode, PolicyDrawer } from './PolicyDrawer';

/** Named sets of permissions that roles attach and API tokens follow. */
export function Policies() {
  const { t } = useTranslation('iam');
  const qc = useQueryClient();
  const scope = useSessionPermissionScope();
  const manageAccess = useActionAccess({
    permission: scope === 'platform' ? 'sys.administrators.manage' : 'iam.policies.manage',
  });
  const deleteAccess = useActionAccess({
    permission: scope === 'platform' ? 'sys.administrators.manage' : 'iam.policies.delete',
  });
  const catalogQuery = useIamPermissionCatalog();
  const [drawer, setDrawer] = React.useState<PolicyDrawerMode | null>(null);
  const [removing, setRemoving] = React.useState<policiesApi.IamPolicy | null>(null);

  const query = useQuery({
    queryKey: policiesApi.POLICIES_QUERY_KEY,
    queryFn: policiesApi.list,
  });
  const rows = query.data ?? [];

  const state = productStateFor(
    queryStateFor({
      isLoading: query.isLoading || catalogQuery.isLoading,
      isError: query.isError || catalogQuery.isError,
      data: rows,
    }),
    {
      error: query.error ?? catalogQuery.error,
      emptyTitle: t('policies.empty_title'),
      emptyDescription: t('policies.empty_description'),
    },
  );

  const remove = useMutation({
    mutationFn: (policy: policiesApi.IamPolicy) => policiesApi.remove(policy.id),
    onSuccess: async () => {
      toast.success(t('policies.toast_deleted'));
      setRemoving(null);
      await qc.invalidateQueries({ queryKey: policiesApi.POLICIES_QUERY_KEY });
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const open = (policy: policiesApi.IamPolicy) =>
    setDrawer(
      policy.builtin || manageAccess.disabled
        ? { kind: 'view', policy }
        : { kind: 'edit', policy },
    );

  return (
    <>
      <IamListPage
        title={t('policies.title')}
        subtitle={t('policies.subtitle')}
        toolbar={
          <ChromeButton
            variant="primary"
            onClick={() => setDrawer({ kind: 'create' })}
            disabled={manageAccess.disabled}
            disabledReason={manageAccess.reason}
          >
            <Plus className="h-3 w-3" />
            {t('policies.new_policy')}
          </ChromeButton>
        }
        state={state}
      >
        {/* Fixed columns: the description takes what the others leave, cut to two lines. */}
        <DataTable
          className="table-fixed"
          rows={rows}
          rowKey={(row) => row.id}
          onRowClick={open}
          emptyLabel={t('policies.empty_title')}
          columns={[
            {
              key: 'name',
              header: t('policies.columns.name'),
              cell: (row) => (
                <div className="min-w-0">
                  <div className="truncate font-sans text-sm text-tx-0">{policyName(row, t)}</div>
                  <div className="truncate font-sans text-xs text-tx-3">{row.key}</div>
                </div>
              ),
              width: 180,
            },
            {
              key: 'type',
              header: t('policies.columns.type'),
              cell: (row) => (
                <span className={cn('text-xs', row.builtin ? 'text-tx-2' : 'text-blue')}>
                  {row.builtin ? t('policies.type_builtin') : t('policies.type_custom')}
                </span>
              ),
              width: 72,
            },
            {
              key: 'description',
              header: t('policies.columns.description'),
              cell: (row) => (
                <span
                  className="line-clamp-2 whitespace-normal text-xs text-tx-2"
                  title={policyDescription(row, t)}
                >
                  {policyDescription(row, t) || '-'}
                </span>
              ),
            },
            {
              key: 'permissions',
              header: t('policies.columns.permissions'),
              cell: (row) => (
                <PermissionSummary
                  catalog={catalogQuery.data}
                  permissions={row.permissions}
                  scope={scope}
                />
              ),
              width: 300,
            },
            {
              key: 'usage',
              header: t('policies.columns.usage'),
              cell: (row) => <PolicyUsage usage={row.usage} />,
              width: 120,
            },
            {
              key: 'actions',
              header: t('policies.columns.actions'),
              width: 110,
              cell: (row) => (
                <div className="flex items-center gap-1">
                  {row.builtin || manageAccess.disabled ? (
                    <IconButton
                      onClick={(event) => {
                        event.stopPropagation();
                        setDrawer({ kind: 'view', policy: row });
                      }}
                      aria-label={t('policies.view_policy', { name: policyName(row, t) })}
                    >
                      <Eye className="h-3 w-3" />
                    </IconButton>
                  ) : (
                    <IconButton
                      onClick={(event) => {
                        event.stopPropagation();
                        setDrawer({ kind: 'edit', policy: row });
                      }}
                      aria-label={t('policies.edit_policy', { name: policyName(row, t) })}
                    >
                      <Pencil className="h-3 w-3" />
                    </IconButton>
                  )}
                  <IconButton
                    disabled={manageAccess.disabled}
                    disabledReason={manageAccess.reason}
                    onClick={(event) => {
                      event.stopPropagation();
                      setDrawer({ kind: 'copy', policy: row });
                    }}
                    aria-label={t('policies.copy_policy', { name: policyName(row, t) })}
                  >
                    <Copy className="h-3 w-3" />
                  </IconButton>
                  <IconButton
                    disabled={deleteAccess.disabled || row.builtin}
                    disabledReason={
                      row.builtin ? t('policies.builtin_read_only') : deleteAccess.reason
                    }
                    onClick={(event) => {
                      event.stopPropagation();
                      setRemoving(row);
                    }}
                    className="enabled:hover:bg-red-dim enabled:hover:text-red-soft"
                    aria-label={t('policies.delete_policy', { name: policyName(row, t) })}
                  >
                    <Trash2 className="h-3 w-3" />
                  </IconButton>
                </div>
              ),
            },
          ]}
        />
      </IamListPage>

      <PolicyDrawer
        mode={drawer}
        catalog={catalogQuery.data}
        policies={rows}
        access={manageAccess}
        onClose={() => setDrawer(null)}
        onCopy={(policy) => setDrawer({ kind: 'copy', policy })}
      />

      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(next) => !next && setRemoving(null)}
        destructive
        title={t('policies.delete_confirm_title')}
        description={
          removing
            ? removing.usage.total > 0
              ? t('policies.delete_in_use', {
                  name: removing.name,
                  roles: removing.usage.roles,
                  tokens: removing.usage.tokens,
                })
              : t('policies.delete_confirm_description', { name: removing.name })
            : undefined
        }
        confirmLabel={t('policies.delete')}
        busy={remove.isPending}
        disabled={deleteAccess.disabled || Boolean(removing && removing.usage.total > 0)}
        disabledReason={
          removing && removing.usage.total > 0
            ? t('policies.delete_blocked')
            : deleteAccess.reason
        }
        onConfirm={() => {
          if (removing && !removing.builtin && deleteAccess.allowed) remove.mutate(removing);
        }}
      />
    </>
  );
}

/** Which roles and tokens use a policy, or a dash. */
function PolicyUsage({ usage }: { usage: policiesApi.PolicyUsage }) {
  const { t } = useTranslation('iam');

  if (usage.total === 0) return <span className="text-xs text-tx-3">-</span>;

  return (
    <span className="whitespace-nowrap text-xs tabular-nums text-tx-1">
      {[
        usage.roles > 0 ? t('policies.usage_roles', { count: usage.roles }) : null,
        usage.tokens > 0 ? t('policies.usage_tokens', { count: usage.tokens }) : null,
      ]
        .filter(Boolean)
        .join(' · ')}
    </span>
  );
}

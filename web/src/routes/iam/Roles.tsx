import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import type { TFunction } from 'i18next';
import { Pencil, Plus, Trash2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog, DataTable } from '@/admin';
import * as policiesApi from '@/api/policies';
import * as rolesApi from '@/api/roles';
import { toApiError } from '@/lib/http';
import { useSessionPermissionScope } from '@/product/access';
import {
  type ActionAccess,
  useActionAccess,
} from '@/product/actionAccess';
import { useIamPermissionCatalog } from '@/product/iamCatalog';
import {
  permissionDefinitionsByDomain,
  type PermissionCatalog,
} from '@/product/permissions';
import { productStateFor } from '@/product/states';
import { ChromeButton, IconButton, Pill } from '@/shell/chrome';
import {
  FormDrawer,
  FormField,
  FormInput,
  FormSection,
  FormSubmitFooter,
  FormTextarea,
} from '@/shell/FormDrawer';
import { queryStateFor } from '@/shell/query/State';
import { toast } from '@/shell/ui/sonner';

import { IamListPage } from './IamLayout';
import { PermissionPicker } from './PermissionPicker';
import { PermissionSummary } from './PermissionSummary';
import { policyName } from './Policies/labels';
import { PolicyChecklist } from './Policies/PolicyChecklist';

export function Roles() {
  const { t } = useTranslation('iam');
  const qc = useQueryClient();
  const [creating, setCreating] = React.useState(false);
  const [editing, setEditing] = React.useState<rolesApi.IamRole | null>(null);
  const [removing, setRemoving] = React.useState<rolesApi.IamRole | null>(null);
  const scope = useSessionPermissionScope();
  const manageAccess = useActionAccess({
    permission: scope === 'platform' ? 'sys.administrators.manage' : 'iam.roles.manage',
  });
  const deleteAccess = useActionAccess({
    permission: scope === 'platform' ? 'sys.administrators.manage' : 'iam.roles.delete',
  });
  const catalogQuery = useIamPermissionCatalog();
  const policiesQuery = useQuery({
    queryKey: policiesApi.POLICIES_QUERY_KEY,
    queryFn: policiesApi.list,
  });
  const policies = policiesQuery.data ?? [];

  const q = useQuery({
    queryKey: ['iam', 'roles'],
    queryFn: () => rolesApi.list(),
  });
  const rows = q.data ?? [];
  const state = productStateFor(queryStateFor({
    isLoading: q.isLoading || catalogQuery.isLoading || policiesQuery.isLoading,
    isError: q.isError || catalogQuery.isError || policiesQuery.isError,
    data: rows,
  }), {
    error: q.error ?? catalogQuery.error ?? policiesQuery.error,
    emptyTitle: t('roles.empty_title'),
    emptyDescription: t('roles.empty_description'),
  });

  const remove = useMutation({
    mutationFn: (role: rolesApi.IamRole) => rolesApi.remove(role.id),
    onSuccess: async () => {
      toast.success(t('roles.toast_deleted'));
      setRemoving(null);
      await qc.invalidateQueries({ queryKey: ['iam', 'roles'] });
    },
    onError: (err) => toast.error(toApiError(err).message),
  });

  return (
    <>
      <IamListPage
        title={t('roles.title')}
        subtitle={t('roles.subtitle') as string}
        toolbar={
          <ChromeButton
            variant="primary"
            onClick={() => setCreating(true)}
            disabled={manageAccess.disabled}
            disabledReason={manageAccess.reason}
          >
            <Plus className="h-3 w-3" />
            {t('roles.new_role')}
          </ChromeButton>
        }
        state={state}
      >
        <DataTable
          rows={rows}
          rowKey={(row) => row.id}
          emptyLabel={t('roles.empty_title')}
          columns={[
            {
              key: 'name',
              header: t('roles.columns.name'),
              cell: (row) => (
                <div className="min-w-0">
                  <div className="overflow-hidden text-ellipsis whitespace-nowrap font-sans text-xs text-tx-0">
                    {row.name}
                  </div>
                  <div className="overflow-hidden text-ellipsis whitespace-nowrap font-sans text-xs text-tx-3">
                    {row.key}
                  </div>
                </div>
              ),
              width: 230,
            },
            {
              key: 'type',
              header: t('roles.columns.type'),
              cell: (row) => (
                <Pill tone={row.builtin ? 'dim' : 'blue'}>
                  {row.builtin ? t('roles.type_builtin') : t('roles.type_custom')}
                </Pill>
              ),
              width: 110,
            },
            {
              key: 'description',
              header: t('roles.columns.description'),
              cell: (row) => roleDescription(row, t) || '-',
              className: 'max-w-[320px]',
            },
            {
              key: 'policies',
              header: t('roles.columns.policies'),
              cell: (row) => <AttachedPolicies ids={row.policyIds} policies={policies} />,
              width: 200,
            },
            {
              key: 'permissions',
              header: t('roles.columns.permissions'),
              cell: (row) => (
                <PermissionSummary
                  catalog={catalogQuery.data}
                  permissions={row.effectivePermissions}
                  scope={scope}
                />
              ),
              width: 280,
            },
            {
              key: 'usage',
              header: t('roles.columns.usage'),
              width: 105,
              cell: (row) => (
                <span
                  title={t('roles.usage_breakdown', {
                    memberships: row.usage.memberships,
                    service_accounts: row.usage.serviceAccounts,
                    invitations: row.usage.invitations,
                    bindings: row.usage.bindings,
                  })}
                >
                  <Pill tone={row.usage.total > 0 ? 'orange' : 'dim'}>
                    {row.usage.total}
                  </Pill>
                </span>
              ),
            },
            {
              key: 'actions',
              header: t('roles.columns.actions'),
              width: 120,
              cell: (row) => {
                const disabled = manageAccess.disabled || row.builtin;
                const reason = row.builtin
                  ? t('roles.builtin_read_only')
                  : manageAccess.reason;
                return (
                  <div
                    className="flex items-center gap-1"
                    onClick={(event) => event.stopPropagation()}
                  >
                    <IconButton
                      disabled={disabled}
                      disabledReason={reason}
                      onClick={(event) => {
                        event.stopPropagation();
                        setEditing(row);
                      }}
                      aria-label={t('roles.edit_role', { name: row.name })}
                    >
                      <Pencil className="h-3 w-3" />
                    </IconButton>
                    <IconButton
                      disabled={deleteAccess.disabled || row.builtin}
                      disabledReason={row.builtin ? t('roles.builtin_read_only') : deleteAccess.reason}
                      onClick={(event) => {
                        event.stopPropagation();
                        setRemoving(row);
                      }}
                      className="enabled:hover:bg-red-dim enabled:hover:text-red-soft"
                      aria-label={t('roles.delete_role', { name: row.name })}
                    >
                      <Trash2 className="h-3 w-3" />
                    </IconButton>
                  </div>
                );
              },
            },
          ]}
        />
      </IamListPage>

      <RoleDrawer
        catalog={catalogQuery.data}
        policies={policies}
        open={creating}
        access={manageAccess}
        onClose={() => setCreating(false)}
      />
      <RoleDrawer
        catalog={catalogQuery.data}
        policies={policies}
        role={editing}
        open={editing !== null}
        access={manageAccess}
        onClose={() => setEditing(null)}
      />

      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(open) => !open && setRemoving(null)}
        destructive
        title={t('roles.delete_confirm_title')}
        description={removing ? t('roles.delete_confirm_description', {
          name: removing.name,
          usage: removing.usage.total,
        }) : undefined}
        confirmLabel={t('roles.delete')}
        busy={remove.isPending}
        disabled={deleteAccess.disabled || Boolean(removing?.builtin)}
        disabledReason={
          removing?.builtin
            ? t('roles.builtin_read_only')
            : deleteAccess.reason
        }
        onConfirm={() => {
          if (removing && !removing.builtin && deleteAccess.allowed) {
            remove.mutate(removing);
          }
        }}
      />
    </>
  );
}

/** The policies a role attaches, by name: the first two, and how many more. */
function AttachedPolicies({
  ids,
  policies,
}: {
  ids: readonly string[];
  policies: readonly policiesApi.IamPolicy[];
}) {
  const { t } = useTranslation('iam');

  if (ids.length === 0) return <span className="text-tx-3">-</span>;

  const names = ids.map((id) => {
    const policy = policies.find((candidate) => candidate.id === id);
    return policy ? policyName(policy, t) : id;
  });
  const shown = names.slice(0, 2).join('、');
  const rest = names.length - 2;

  return (
    <span
      className="block max-w-[200px] truncate text-xs text-tx-1"
      title={names.join('、')}
      data-role-policies
    >
      {rest > 0 ? t('roles.policies_more', { shown, count: rest }) : shown}
    </span>
  );
}

function RoleDrawer({
  catalog,
  policies,
  role,
  open,
  access,
  onClose,
}: {
  catalog: PermissionCatalog | undefined;
  policies: readonly policiesApi.IamPolicy[];
  role?: rolesApi.IamRole | null;
  open: boolean;
  access: ActionAccess;
  onClose: () => void;
}) {
  const { t } = useTranslation('iam');
  const { t: tc } = useTranslation('common');
  const qc = useQueryClient();
  const [key, setKey] = React.useState('');
  const [name, setName] = React.useState('');
  const [description, setDescription] = React.useState('');
  const [permissions, setPermissions] = React.useState<rolesApi.PermissionKey[]>([]);
  const [policyIds, setPolicyIds] = React.useState<string[]>([]);
  const [keyEdited, setKeyEdited] = React.useState(false);
  const isEdit = Boolean(role);
  const scope = useSessionPermissionScope();
  const permissionGroups = permissionDefinitionsByDomain(catalog, scope);

  // What the attached policies grant, each permission with the policies that grant it.
  const inherited = React.useMemo(() => {
    const sources = new Map<string, string[]>();

    for (const id of policyIds) {
      const policy = policies.find((candidate) => candidate.id === id);
      if (!policy) continue;

      for (const permission of policy.permissions) {
        sources.set(permission, [...(sources.get(permission) ?? []), policyName(policy, t)]);
      }
    }

    return new Map([...sources].map(([permission, names]) => [permission, names.join('、')]));
  }, [policies, policyIds, t]);

  React.useEffect(() => {
    if (!open) return;
    setKey(role?.key ?? '');
    setName(role?.name ?? '');
    setDescription(role?.description ?? '');
    setPermissions(role?.permissions ?? []);
    setPolicyIds(role?.policyIds ?? []);
    setKeyEdited(Boolean(role));
  }, [open, role]);

  React.useEffect(() => {
    if (!open || isEdit || keyEdited) return;
    setKey(roleKeyFromName(name));
  }, [isEdit, keyEdited, name, open]);

  const save = useMutation({
    mutationFn: () => {
      if (role) {
        return rolesApi.update(role.id, {
          name,
          description,
          permissions,
          policyIds: policyIds,
        });
      }
      return rolesApi.create({
        key,
        name,
        description,
        permissions,
        policyIds: policyIds,
      });
    },
    onSuccess: async () => {
      toast.success(isEdit ? t('roles.toast_updated') : t('roles.toast_created'));
      onClose();
      await Promise.all([
        qc.invalidateQueries({ queryKey: ['iam', 'roles'] }),
        qc.invalidateQueries({ queryKey: policiesApi.POLICIES_QUERY_KEY }),
      ]);
    },
    onError: (err) => toast.error(toApiError(err).message),
  });
  const invalid = key.trim().length === 0 || name.trim().length === 0;
  const dirty =
    !role ||
    name !== role.name ||
    description !== role.description ||
    [...permissions].sort().join('\n') !==
      [...role.permissions].sort().join('\n') ||
    [...policyIds].sort().join('\n') !== [...role.policyIds].sort().join('\n');
  const controlsDisabled = access.disabled || save.isPending;
  const submitDisabled = access.disabled || Boolean(role && !dirty);

  return (
    <FormDrawer
      open={open}
      onOpenChange={(next) => !next && onClose()}
      title={isEdit ? t('roles.drawer_edit_title') : t('roles.drawer_create_title')}
      subtitle={t('roles.drawer_subtitle')}
      footer={
        <FormSubmitFooter
          busy={save.isPending}
          disabled={submitDisabled}
          invalid={invalid}
          disabledReason={
            access.reason ??
            (invalid
              ? tc('access.form_invalid')
              : !dirty
                ? tc('access.no_changes')
                : undefined)
          }
          onCancel={onClose}
          submitLabel={isEdit ? t('roles.save_role') : t('roles.create_role')}
          formId="role-form"
        />
      }
    >
      <form
        id="role-form"
        onSubmit={(event) => {
          event.preventDefault();
          if (submitDisabled || invalid || save.isPending) return;
          save.mutate();
        }}
      >
        <FormSection title={t('roles.sections.identity')}>
          <FormField label={t('roles.fields.key')} required hint={t('roles.fields.key_hint')}>
            <FormInput
              value={key}
              onChange={(event) => {
                setKeyEdited(true);
                setKey(event.target.value);
              }}
              readOnly={isEdit}
              disabled={controlsDisabled}
              disabledReason={access.reason}
              required
            />
          </FormField>
          <FormField label={t('roles.fields.name')} required>
            <FormInput
              value={name}
              onChange={(event) => setName(event.target.value)}
              disabled={controlsDisabled}
              disabledReason={access.reason}
              required
            />
          </FormField>
          <FormField label={t('roles.fields.description')}>
            <FormTextarea
              value={description}
              onChange={(event) => setDescription(event.target.value)}
              rows={3}
              disabled={controlsDisabled}
              disabledReason={access.reason}
            />
          </FormField>
        </FormSection>
        <FormSection
          title={t('roles.sections.policies')}
          description={t('roles.sections.policies_description')}
        >
          <PolicyChecklist
            policies={policies}
            selected={policyIds}
            onChange={setPolicyIds}
            disabled={controlsDisabled}
            disabledReason={access.reason}
          />
        </FormSection>
        <FormSection
          title={t('roles.sections.own_permissions')}
          description={t('roles.sections.own_permissions_description')}
        >
          <PermissionPicker
            groups={permissionGroups}
            selected={permissions}
            onChange={setPermissions}
            inherited={inherited}
            disabled={controlsDisabled}
            disabledReason={access.reason}
          />
        </FormSection>
      </form>
    </FormDrawer>
  );
}

/** A built-in role describes itself in the interface language; a custom role as written. */
function roleDescription(role: rolesApi.IamRole, t: TFunction<'iam'>): string {
  if (!role.builtin) return role.description;
  return t(`roles.builtin_descriptions.${role.key}`, { defaultValue: role.description });
}

function roleKeyFromName(name: string): string {
  const normalized = name
    .normalize('NFKD')
    .replace(/[\u0300-\u036f]/g, '')
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '_')
    .replace(/^_+|_+$/g, '');
  const prefixed = /^[a-z]/.test(normalized) ? normalized : `role_${normalized}`;
  return (prefixed || 'role').slice(0, 64);
}

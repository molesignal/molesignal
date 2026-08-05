import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Pencil, Plus, Trash2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog, DataTable } from '@/admin';
import * as rolesApi from '@/api/roles';
import { toApiError } from '@/lib/http';
import {
  type ActionAccess,
  useActionAccess,
} from '@/product/actionAccess';
import { useIamPermissionCatalog } from '@/product/iamCatalog';
import {
  permissionDefinition,
  permissionDefinitionsByDomain,
  type PermissionCatalog,
} from '@/product/permissions';
import { productStateFor } from '@/product/states';
import { ChromeButton, IconButton, Pill } from '@/shell/chrome';
import {
  FormChecklist,
  FormDrawer,
  FormField,
  FormInput,
  FormSelect,
  FormSection,
  FormSubmitFooter,
  FormTextarea,
} from '@/shell/FormDrawer';
import { queryStateFor } from '@/shell/query/State';
import { toast } from '@/shell/ui/sonner';

import { IamListPage } from './IamLayout';

export function Roles() {
  const { t } = useTranslation('iam');
  const qc = useQueryClient();
  const [creating, setCreating] = React.useState(false);
  const [editing, setEditing] = React.useState<rolesApi.IamRole | null>(null);
  const [removing, setRemoving] = React.useState<rolesApi.IamRole | null>(null);
  const manageAccess = useActionAccess({
    permission: 'iam.roles.manage',
  });
  const catalogQuery = useIamPermissionCatalog();

  const q = useQuery({
    queryKey: ['iam', 'roles'],
    queryFn: () => rolesApi.list(),
  });
  const rows = q.data ?? [];
  const state = productStateFor(queryStateFor({
    isLoading: q.isLoading || catalogQuery.isLoading,
    isError: q.isError || catalogQuery.isError,
    data: rows,
  }), {
    error: q.error ?? catalogQuery.error,
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
              cell: (row) => row.description || '-',
              className: 'max-w-[320px]',
            },
            {
              key: 'permissions',
              header: t('roles.columns.permissions'),
              cell: (row) => (
                <PermissionSummary
                  catalog={catalogQuery.data}
                  permissions={row.permissions}
                />
              ),
              width: 300,
            },
            {
              key: 'usage',
              header: t('roles.columns.usage'),
              width: 105,
              cell: (row) => (
                <span
                  title={t('roles.usage_breakdown', {
                    memberships: row.usage.memberships,
                    api_tokens: row.usage.api_tokens,
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
                      disabled={disabled}
                      disabledReason={reason}
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
        open={creating}
        access={manageAccess}
        onClose={() => setCreating(false)}
      />
      <RoleDrawer
        catalog={catalogQuery.data}
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
        disabled={manageAccess.disabled || Boolean(removing?.builtin)}
        disabledReason={
          removing?.builtin
            ? t('roles.builtin_read_only')
            : manageAccess.reason
        }
        onConfirm={() => {
          if (removing && !removing.builtin && manageAccess.allowed) {
            remove.mutate(removing);
          }
        }}
      />
    </>
  );
}

function PermissionSummary({
  catalog,
  permissions,
}: {
  catalog: PermissionCatalog | undefined;
  permissions: rolesApi.PermissionKey[];
}) {
  const { t } = useTranslation('iam');
  if (permissions.length === 0) {
    return <span className="text-tx-3">-</span>;
  }
  const visible = permissions.slice(0, 3);
  return (
    <div className="flex min-w-0 items-center gap-1 overflow-hidden">
      {visible.map((permission) => {
        const option = permissionDefinition(catalog, permission);
        return (
          <Pill key={permission} tone="neutral">
            {option ? t(option.label_key) : permission}
          </Pill>
        );
      })}
      {permissions.length > visible.length && (
        <Pill tone="dim">+{permissions.length - visible.length}</Pill>
      )}
    </div>
  );
}

function RoleDrawer({
  catalog,
  role,
  open,
  access,
  onClose,
}: {
  catalog: PermissionCatalog | undefined;
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
  const [keyEdited, setKeyEdited] = React.useState(false);
  const isEdit = Boolean(role);
  const permissionGroups = permissionDefinitionsByDomain(catalog);

  React.useEffect(() => {
    if (!open) return;
    setKey(role?.key ?? '');
    setName(role?.name ?? '');
    setDescription(role?.description ?? '');
    setPermissions(role?.permissions ?? []);
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
        });
      }
      return rolesApi.create({
        key,
        name,
        description,
        permissions,
      });
    },
    onSuccess: async () => {
      toast.success(isEdit ? t('roles.toast_updated') : t('roles.toast_created'));
      onClose();
      await qc.invalidateQueries({ queryKey: ['iam', 'roles'] });
    },
    onError: (err) => toast.error(toApiError(err).message),
  });
  const invalid = key.trim().length === 0 || name.trim().length === 0;
  const dirty =
    !role ||
    name !== role.name ||
    description !== role.description ||
    [...permissions].sort().join('\n') !==
      [...role.permissions].sort().join('\n');
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
          title={t('roles.sections.permissions')}
          description={t('roles.sections.permissions_description')}
        >
          <FormField
            label={t('roles.fields.bundle')}
            hint={t('roles.fields.bundle_hint')}
          >
            <FormSelect
              value=""
              disabled={controlsDisabled}
              disabledReason={access.reason}
              onChange={(value) => {
                const bundle = catalog?.bundles.find(
                  (candidate) => candidate.key === value,
                );
                if (bundle) setPermissions([...bundle.permissions]);
              }}
              options={[
                { value: '', label: t('roles.fields.bundle_placeholder') },
                ...(catalog?.bundles ?? []).map((bundle) => ({
                  value: bundle.key,
                  label: t(bundle.label_key),
                })),
              ]}
            />
          </FormField>
          <div className="space-y-4">
            {permissionGroups.map((group) => (
              <section
                key={group.domain}
                className="rounded-md border border-border bg-bg-1 p-3"
              >
                <h4 className="mb-2 font-sans text-xs font-semibold text-tx-1">
                  {t(`permission_domains.${group.domain}`)}
                </h4>
                <FormChecklist<rolesApi.PermissionKey>
                  selected={permissions}
                  onChange={setPermissions}
                  disabled={controlsDisabled}
                  disabledReason={access.reason}
                  options={group.permissions.map((permission) => ({
                    value: permission.key,
                    label: t(permission.label_key),
                    hint: t(permission.description_key),
                  }))}
                />
              </section>
            ))}
          </div>
        </FormSection>
      </form>
    </FormDrawer>
  );
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

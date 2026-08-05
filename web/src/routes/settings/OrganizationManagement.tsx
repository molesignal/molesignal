import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog, DataTable, PageHeader } from '@/admin';
import * as orgsApi from '@/api/orgs';
import { toApiError } from '@/lib/http';
import {
  type ActionAccess,
  useActionAccess,
} from '@/product/actionAccess';
import { ProductState, productStateFor } from '@/product/states';
import { ChromeButton, Pill } from '@/shell/chrome';
import {
  FormDrawer,
  FormField,
  FormInput,
  FormRow,
  FormSection,
  FormSubmitFooter,
} from '@/shell/FormDrawer';
import { queryStateFor } from '@/shell/query/State';
import { toast } from '@/shell/ui/sonner';
import { useOrgStore } from '@/stores/useOrgStore';

import { CopyableValue, SectionBody } from './_atoms';

export function OrganizationManagement({ embedded = false }: { embedded?: boolean } = {}) {
  const { t } = useTranslation('settings-admin');
  const { t: tc } = useTranslation('common');
  const qc = useQueryClient();
  const currentOrgId = useOrgStore((s) => s.currentOrgId);
  const setOrgs = useOrgStore((s) => s.setOrgs);
  const upsertOrg = useOrgStore((s) => s.upsertOrg);
  const removeOrgFromStore = useOrgStore((s) => s.removeOrg);
  const manageAccess = useActionAccess({
    permission: 'sys.organizations.manage',
  });
  const [creating, setCreating] = React.useState(false);
  const [editing, setEditing] = React.useState<orgsApi.Org | null>(null);
  const [removing, setRemoving] = React.useState<orgsApi.Org | null>(null);
  const [statusTarget, setStatusTarget] = React.useState<orgsApi.Org | null>(null);

  const q = useQuery({ queryKey: ['orgs', 'list'], queryFn: () => orgsApi.listOrgs() });
  const rows = q.data ?? [];

  React.useEffect(() => {
    if (q.data) setOrgs(q.data);
  }, [q.data, setOrgs]);

  const state = queryStateFor({ isLoading: q.isLoading, isError: q.isError, data: rows });
  const pageState = productStateFor(state, {
    error: q.error,
    emptyTitle: t('organization_management.empty_title'),
    emptyDescription: t('organization_management.empty_description'),
  });

  const cacheOrg = (org: orgsApi.Org) => {
    upsertOrg(org);
    qc.setQueryData<orgsApi.Org[]>(['orgs', 'list'], (prev) => mergeOrg(prev, org));
    qc.setQueryData<orgsApi.Org[]>(['iam', 'orgs'], (prev) => mergeOrg(prev, org));
  };

  const changeStatus = useMutation({
    mutationFn: (org: orgsApi.Org) => orgsApi.setOrgDisabled(org.id, !org.disabled),
    onSuccess: (org) => {
      cacheOrg(org);
      toast.success(t(org.disabled ? 'organization_management.toast_disabled' : 'organization_management.toast_enabled'));
      setStatusTarget(null);
    },
    onError: (err) => toast.error(toApiError(err).message),
  });

  const remove = useMutation({
    mutationFn: (id: string) => orgsApi.removeOrg(id),
    onSuccess: (_data, id) => {
      toast.success(t('organization_management.toast_deleted'));
      removeOrgFromStore(id);
      qc.setQueryData<orgsApi.Org[]>(['orgs', 'list'], (prev) => (prev ?? []).filter((org) => org.id !== id));
      qc.setQueryData<orgsApi.Org[]>(['iam', 'orgs'], (prev) => (prev ?? []).filter((org) => org.id !== id));
      setRemoving(null);
    },
    onError: (err) => toast.error(toApiError(err).message),
  });

  return (
    <>
      {embedded ? (
        <div className="flex items-center justify-between border-t border-bd-0 px-6 pt-6">
          <div>
            <div className="font-sans text-sm font-display-strong text-tx-0">
              {t('organization_management.title')}
            </div>
            <div className="mt-1 font-sans text-xs text-tx-2">
              {t('organization_management.subtitle')}
            </div>
          </div>
          <ChromeButton
            variant="primary"
            onClick={() => {
              if (manageAccess.allowed) setCreating(true);
            }}
            disabled={manageAccess.disabled}
            disabledReason={manageAccess.reason}
          >
            {t('organization_management.new_org')}
          </ChromeButton>
        </div>
      ) : (
        <PageHeader
          title={t('organization_management.title')}
          subtitle={t('organization_management.subtitle') as string}
          actions={
            <ChromeButton
              variant="primary"
              onClick={() => {
                if (manageAccess.allowed) setCreating(true);
              }}
              disabled={manageAccess.disabled}
              disabledReason={manageAccess.reason}
            >
              {t('organization_management.new_org')}
            </ChromeButton>
          }
        />
      )}
      <CreateDrawer
        access={manageAccess}
        open={creating}
        onClose={() => setCreating(false)}
        onCreated={(org) => {
          cacheOrg(org);
          void qc.invalidateQueries({ queryKey: ['orgs', 'list'] });
        }}
      />
      <EditDrawer
        access={manageAccess}
        org={editing}
        onClose={() => setEditing(null)}
        onUpdated={(org) => {
          cacheOrg(org);
          setEditing(null);
        }}
      />
      <ConfirmDialog
        open={statusTarget !== null}
        onOpenChange={(value) => !value && setStatusTarget(null)}
        destructive={statusTarget?.disabled === false}
        title={t(statusTarget?.disabled ? 'organization_management.enable_title' : 'organization_management.disable_title')}
        description={t(statusTarget?.disabled ? 'organization_management.enable_description' : 'organization_management.disable_description', { name: statusTarget?.name ?? '' })}
        confirmLabel={tc(statusTarget?.disabled ? 'actions.enable' : 'actions.disable')}
        busy={changeStatus.isPending}
        onConfirm={() => statusTarget && changeStatus.mutate(statusTarget)}
      />
      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(value) => !value && setRemoving(null)}
        destructive
        title={t('organization_management.delete_title')}
        description={t('organization_management.delete_description', { name: removing?.name ?? '' })}
        confirmLabel={tc('actions.delete')}
        busy={remove.isPending}
        disabled={manageAccess.disabled}
        disabledReason={manageAccess.reason}
        onConfirm={() => {
          if (manageAccess.allowed && removing) remove.mutate(removing.id);
        }}
      />
      <SectionBody className={embedded ? 'pt-3' : ''}>
        {pageState ? (
          <ProductState {...pageState} />
        ) : (
          <DataTable
            rows={rows}
            rowKey={(r) => r.id}
            columns={[
              {
                key: 'name',
                header: t('organization_management.columns.name'),
                cell: (r) => (
                  <span className="flex items-center gap-2 text-tx-0">
                    {r.name}
                    {r.id === currentOrgId && (
                      <Pill tone="orange">{t('organization_management.current_badge')}</Pill>
                    )}
                  </span>
                ),
              },
              {
                key: 'slug',
                header: t('organization_management.columns.slug'),
                cell: (r) => r.slug ?? '—',
                width: 200,
              },
              {
                key: 'status',
                header: t('organization_management.columns.status'),
                cell: (r) => r.disabled
                  ? <Pill tone="dim">{tc('status.disabled')}</Pill>
                  : <Pill tone="green">{tc('status.enabled')}</Pill>,
                width: 110,
              },
              {
                key: 'id',
                header: t('organization_management.columns.id'),
                cell: (r) => (
                  <CopyableValue
                    value={r.id}
                    copyLabel={t('organization.copy_id')}
                    copiedLabel={t('organization.copied')}
                  />
                ),
                width: 280,
              },
              {
                key: 'actions',
                header: '',
                width: 260,
                cell: (r: orgsApi.Org) => {
                  const editDisabled = manageAccess.disabled || Boolean(r.system);
                  const tenantOrganizations = rows.filter((org) => !org.system);
                  const enabledTenantCount = tenantOrganizations.filter((org) => !org.disabled).length;
                  const lastEnabled = !r.disabled && enabledTenantCount <= 1;
                  const statusDisabled = manageAccess.disabled || Boolean(r.system) || lastEnabled;
                  const statusReason = r.system
                    ? tc('access.system_workspace')
                    : lastEnabled
                      ? t('organization_management.last_enabled')
                      : manageAccess.reason;
                  const deleteReason = r.system
                    ? tc('access.system_workspace')
                    : r.id === currentOrgId
                      ? tc('access.current_workspace')
                      : tenantOrganizations.length <= 1 || lastEnabled
                        ? tc('access.last_workspace')
                        : manageAccess.reason;
                  const deleteDisabled =
                    manageAccess.disabled ||
                    Boolean(r.system) ||
                    r.id === currentOrgId ||
                    tenantOrganizations.length <= 1 ||
                    lastEnabled;
                  return (
                    <div
                      className="flex items-center justify-end gap-1"
                      onClick={(event) => event.stopPropagation()}
                    >
                      <ChromeButton
                        variant="ghost"
                        size="sm"
                        disabled={statusDisabled}
                        disabledReason={statusReason}
                        onClick={() => !statusDisabled && setStatusTarget(r)}
                        className={!r.disabled ? 'enabled:hover:text-red-soft' : undefined}
                      >
                        {tc(r.disabled ? 'actions.enable' : 'actions.disable')}
                      </ChromeButton>
                      <ChromeButton
                        variant="ghost"
                        size="sm"
                        disabled={editDisabled}
                        disabledReason={
                          r.system
                            ? tc('access.system_workspace')
                            : manageAccess.reason
                        }
                        onClick={() => {
                          if (!editDisabled) setEditing(r);
                        }}
                      >
                        {tc('actions.edit')}
                      </ChromeButton>
                      <ChromeButton
                        variant="ghost"
                        size="sm"
                        disabled={deleteDisabled}
                        disabledReason={deleteReason}
                        onClick={() => {
                          if (!deleteDisabled) setRemoving(r);
                        }}
                        className="enabled:hover:text-red-soft"
                      >
                        {tc('actions.delete')}
                      </ChromeButton>
                    </div>
                  );
                },
              },
            ]}
          />
        )}
      </SectionBody>
    </>
  );
}


function EditDrawer({
  access,
  org,
  onClose,
  onUpdated,
}: {
  access: ActionAccess;
  org: orgsApi.Org | null;
  onClose: () => void;
  onUpdated: (org: orgsApi.Org) => void;
}) {
  const { t } = useTranslation('settings-admin');
  const { t: tc } = useTranslation('common');
  const [name, setName] = React.useState('');

  React.useEffect(() => {
    if (!org) return;
    setName(org.name);
  }, [org]);
  const dirty = Boolean(org && name.trim() !== org.name);
  const invalid = name.trim().length === 0;

  const update = useMutation({
    mutationFn: () => {
      if (!org) throw new Error('missing org');
      return orgsApi.updateOrg(org.id, { name: name.trim() });
    },
    onSuccess: (updated) => {
      toast.success(tc('status.updated'));
      onUpdated(updated);
    },
    onError: (err) => toast.error(toApiError(err).message),
  });

  return (
    <FormDrawer
      open={org !== null}
      onOpenChange={(value) => !value && onClose()}
      title={t('organization.edit_drawer_title')}
      width={520}
      footer={
        <FormSubmitFooter
          busy={update.isPending}
          disabled={access.disabled || !dirty}
          invalid={invalid}
          disabledReason={
            access.reason ??
            (invalid
              ? tc('access.form_invalid')
              : t('organization.no_changes'))
          }
          onCancel={onClose}
          submitLabel={tc('actions.save')}
          formId="org-edit-form"
        />
      }
    >
      <form
        id="org-edit-form"
        onSubmit={(event) => {
          event.preventDefault();
          if (access.allowed && dirty && !invalid) update.mutate();
        }}
      >
        <FormSection>
          <FormRow>
            <FormField label={t('organization.name')} required>
              <FormInput
                value={name}
                onChange={(event) => setName(event.target.value)}
                disabled={access.disabled || update.isPending}
                disabledReason={access.reason}
                required
              />
            </FormField>
            <FormField label={t('organization.slug')} required>
              <FormInput
                value={org?.slug ?? ''}
                readOnly
                aria-readonly="true"
                title={tc('access.immutable')}
              />
            </FormField>
          </FormRow>
        </FormSection>
      </form>
    </FormDrawer>
  );
}

function CreateDrawer({
  access,
  open,
  onClose,
  onCreated,
}: {
  access: ActionAccess;
  open: boolean;
  onClose: () => void;
  onCreated: (org: orgsApi.Org) => void;
}) {
  const { t } = useTranslation('settings-admin');
  const [name, setName] = React.useState('');
  const [slug, setSlug] = React.useState('');
  const [slugTouched, setSlugTouched] = React.useState(false);

  React.useEffect(() => {
    if (!open) {
      setName('');
      setSlug('');
      setSlugTouched(false);
    }
  }, [open]);

  const create = useMutation({
    mutationFn: () => orgsApi.createOrg({ name: name.trim(), slug: slug.trim() }),
    onSuccess: (org) => {
      toast.success(t('organization_management.toast_created'));
      onCreated(org);
      onClose();
    },
    onError: (err) => toast.error(toApiError(err).message),
  });
  const invalid = name.trim().length === 0 || slug.trim().length === 0;

  return (
    <FormDrawer
      open={open}
      onOpenChange={(v) => !v && onClose()}
      title={t('organization_management.create_drawer_title')}
      subtitle={t('organization_management.create_drawer_subtitle') as string}
      footer={
        <FormSubmitFooter
          busy={create.isPending}
          disabled={access.disabled}
          invalid={invalid}
          disabledReason={
            access.reason ??
            (invalid ? t('organization_management.form_invalid') : undefined)
          }
          onCancel={onClose}
          submitLabel={t('organization_management.new_org')}
          formId="org-create-form"
        />
      }
    >
      <form
        id="org-create-form"
        onSubmit={(e) => {
          e.preventDefault();
          if (access.allowed && !invalid) create.mutate();
        }}
      >
        <FormSection>
          <FormRow>
            <FormField label={t('organization_management.field_name')} required>
              <FormInput
                value={name}
                onChange={(e) => {
                  const next = e.target.value;
                  setName(next);
                  if (!slugTouched) setSlug(slugify(next));
                }}
                disabled={access.disabled || create.isPending}
                disabledReason={access.reason}
                required
              />
            </FormField>
            <FormField label={t('organization_management.field_slug')} required>
              <FormInput
                value={slug}
                onChange={(e) => {
                  setSlugTouched(true);
                  setSlug(slugify(e.target.value));
                }}
                placeholder="example-prod"
                disabled={access.disabled || create.isPending}
                disabledReason={access.reason}
                required
              />
            </FormField>
          </FormRow>
        </FormSection>
      </form>
    </FormDrawer>
  );
}

function mergeOrg(prev: orgsApi.Org[] | undefined, org: orgsApi.Org): orgsApi.Org[] {
  return [...(prev ?? []).filter((item) => item.id !== org.id), org].sort((a, b) =>
    a.name.localeCompare(b.name),
  );
}

function slugify(input: string): string {
  return input
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 48);
}

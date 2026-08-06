import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog, DataTable, PageHeader } from '@/admin';
import * as instanceApi from '@/api/instance';
import * as ssoApi from '@/api/sso';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ProductState, productStateFor } from '@/product/states';
import { ChromeButton, Pill } from '@/shell/chrome';
import { FormDrawer, FormSubmitFooter } from '@/shell/FormDrawer';
import { queryStateFor } from '@/shell/query/State';
import { toast } from '@/shell/ui/sonner';

import {
  applySsoCallbackUrls,
  type ProviderDraft,
  draftFromProvider,
  draftIsInvalid,
  draftToInput,
  emptyDraft,
  resolveSsoCallbackUrls,
} from './model';
import { ProviderForm } from './ProviderForm';
import { formatMicros } from '../../rum/_helpers';
import { SectionBody } from '../_atoms';

export function SsoProviders() {
  const { t } = useTranslation('settings-admin');
  const { t: tc } = useTranslation('common');
  const qc = useQueryClient();
  const manageAccess = useActionAccess({
    permission: 'org.settings.manage',
  });
  const [editing, setEditing] = React.useState<ssoApi.SsoProvider | 'new' | null>(null);
  const [draft, setDraft] = React.useState<ProviderDraft>(emptyDraft());
  const [removing, setRemoving] = React.useState<ssoApi.SsoProvider | null>(null);

  const q = useQuery({ queryKey: ['sso', 'providers'], queryFn: () => ssoApi.list() });
  const instanceQuery = useQuery({
    queryKey: ['instance'],
    queryFn: instanceApi.get,
    staleTime: 300_000,
  });
  const callbackUrls = React.useMemo(
    () =>
      resolveSsoCallbackUrls(
        instanceQuery.data?.external_url,
        window.location.origin,
      ),
    [instanceQuery.data?.external_url],
  );
  const rolesQuery = useQuery({
    queryKey: ['sso', 'provider-roles'],
    queryFn: () => ssoApi.listAssignableRoles(),
    enabled: editing !== null,
  });
  const rows = q.data ?? [];
  const state = queryStateFor({ isLoading: q.isLoading, isError: q.isError, data: rows });
  const pageState = productStateFor(state, {
    error: q.error,
    emptyTitle: t('sso_providers.empty_title'),
    emptyDescription: t('sso_providers.empty_description'),
  });

  const openCreate = () => {
    if (manageAccess.disabled) return;
    setDraft(applySsoCallbackUrls(emptyDraft(), callbackUrls));
    setEditing('new');
  };
  const openEdit = (provider: ssoApi.SsoProvider) => {
    if (manageAccess.disabled) return;
    setDraft(applySsoCallbackUrls(draftFromProvider(provider), callbackUrls));
    setEditing(provider);
  };
  const closeDrawer = () => setEditing(null);

  React.useEffect(() => {
    if (editing === null) return;
    setDraft((current) => applySsoCallbackUrls(current, callbackUrls));
  }, [callbackUrls, editing]);

  const createMutation = useMutation({
    mutationFn: (input: ssoApi.SsoProviderInput) => ssoApi.create(input),
    onSuccess: () => {
      toast.success(t('sso_providers.drawer.toast_created'));
      void qc.invalidateQueries({ queryKey: ['sso', 'providers'] });
      closeDrawer();
    },
    onError: (err) => toast.error(toApiError(err).message),
  });

  const updateMutation = useMutation({
    mutationFn: ({ id, input }: { id: string; input: ssoApi.SsoProviderInput }) =>
      ssoApi.update(id, input),
    onSuccess: () => {
      toast.success(t('sso_providers.drawer.toast_updated'));
      void qc.invalidateQueries({ queryKey: ['sso', 'providers'] });
      closeDrawer();
    },
    onError: (err) => toast.error(toApiError(err).message),
  });

  const removeMutation = useMutation({
    mutationFn: (id: string) => ssoApi.remove(id),
    onSuccess: () => {
      toast.success(t('sso_providers.drawer.toast_deleted'));
      void qc.invalidateQueries({ queryKey: ['sso', 'providers'] });
      setRemoving(null);
    },
    onError: (err) => toast.error(toApiError(err).message),
  });

  // enable/disable are tiny POST-no-body calls; we collapse them into a
  // single mutation parameterised by the next desired state so the table
  // row only ever has one pending state at a time.
  const toggleMutation = useMutation({
    mutationFn: ({ id, enable }: { id: string; enable: boolean }) =>
      enable ? ssoApi.enable(id) : ssoApi.disable(id),
    onSuccess: (_data, variables) => {
      toast.success(
        variables.enable
          ? t('sso_providers.drawer.toast_enabled')
          : t('sso_providers.drawer.toast_disabled'),
      );
      void qc.invalidateQueries({ queryKey: ['sso', 'providers'] });
    },
    onError: (err) => toast.error(toApiError(err).message),
  });

  const submitting = createMutation.isPending || updateMutation.isPending;
  const invalid = draftIsInvalid(draft);
  const dirty =
    editing === 'new' ||
    (editing !== null &&
      JSON.stringify(draftToInput(draft)) !==
        JSON.stringify(draftToInput(draftFromProvider(editing))));
  const controlsDisabled = manageAccess.disabled || submitting;
  const submitDrawer = (event: React.FormEvent) => {
    event.preventDefault();
    if (manageAccess.disabled || invalid || !dirty || submitting) return;
    const input = draftToInput(draft);
    if (editing === 'new') {
      createMutation.mutate(input);
    } else if (editing) {
      updateMutation.mutate({ id: editing.id, input });
    }
  };

  return (
    <>
      <PageHeader
        title={t('sso_providers.title')}
        subtitle={t('sso_providers.subtitle') as string}
        className="bg-transparent"
        actions={
          <ChromeButton
            variant="primary"
            onClick={openCreate}
            disabled={manageAccess.disabled}
            disabledReason={manageAccess.reason}
          >
            {t('sso_providers.new_provider')}
          </ChromeButton>
        }
      />
      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(v) => !v && setRemoving(null)}
        destructive
        title={t('sso_providers.delete_confirm_title')}
        description={t('sso_providers.delete_confirm_description')}
        confirmLabel={t('sso_providers.delete_confirm_label')}
        busy={removeMutation.isPending}
        disabled={manageAccess.disabled}
        disabledReason={manageAccess.reason}
        onConfirm={() => {
          if (removing && manageAccess.allowed) {
            removeMutation.mutate(removing.id);
          }
        }}
      />
      <FormDrawer
        open={editing !== null}
        onOpenChange={(v) => !v && closeDrawer()}
        title={
          editing === 'new'
            ? t('sso_providers.drawer.create_title')
            : t('sso_providers.drawer.edit_title', { name: editing?.name ?? '' })
        }
        subtitle={t('sso_providers.drawer.subtitle') as string}
        footer={
          <FormSubmitFooter
            busy={submitting}
            disabled={manageAccess.disabled || !dirty}
            invalid={invalid}
            disabledReason={
              manageAccess.reason ??
              (invalid
                ? tc('access.form_invalid')
                : !dirty
                  ? tc('access.no_changes')
                  : undefined)
            }
            onCancel={closeDrawer}
            submitLabel={
              editing === 'new'
                ? t('sso_providers.drawer.submit_create')
                : t('sso_providers.drawer.submit_update')
            }
            formId="sso-provider-form"
          />
        }
      >
        <form id="sso-provider-form" onSubmit={submitDrawer}>
          <ProviderForm
            draft={draft}
            roles={rolesQuery.data ?? []}
            rolesLoading={rolesQuery.isLoading}
            rolesError={
              rolesQuery.isError
                ? toApiError(rolesQuery.error).message
                : null
            }
            onRetryRoles={() => {
              void rolesQuery.refetch();
            }}
            onChange={setDraft}
            disabled={controlsDisabled}
          />
        </form>
      </FormDrawer>

      <SectionBody>
        {pageState ? (
          <ProductState {...pageState} />
        ) : (
          <DataTable
            rows={rows}
            rowKey={(r) => r.id}
            columns={[
              {
                key: 'name',
                header: t('sso_providers.columns.name'),
                cell: (r) => <span className="font-semibold text-tx-0">{r.name}</span>,
              },
              {
                key: 'kind',
                header: t('sso_providers.columns.kind'),
                cell: (r) => (
                  <Pill
                    tone={
                      r.kind === 'saml'
                        ? 'purple'
                        : r.kind === 'ldap'
                          ? 'green'
                          : 'blue'
                    }
                  >
                    {t(`sso_providers.kinds.${r.kind}`)}
                  </Pill>
                ),
                width: 110,
              },
              {
                key: 'enabled',
                header: t('sso_providers.columns.enabled'),
                cell: (r) =>
                  r.enabled ? (
                    <Pill tone="green">{tc('status.on')}</Pill>
                  ) : (
                    <Pill tone="dim">{tc('status.off')}</Pill>
                  ),
                width: 90,
              },
              {
                key: 'updated',
                header: t('sso_providers.columns.updated'),
                cell: (r) => (r.updated_at_micros ? formatMicros(r.updated_at_micros) : '—'),
                width: 200,
              },
              {
                key: 'actions',
                header: '',
                width: 220,
                cell: (r) => (
                  <div
                    className="flex gap-2"
                    onClick={(e) => e.stopPropagation()}
                  >
                    <ChromeButton
                      variant="ghost"
                      size="sm"
                      onClick={() => toggleMutation.mutate({ id: r.id, enable: !r.enabled })}
                      disabled={manageAccess.disabled || toggleMutation.isPending}
                      disabledReason={
                        manageAccess.reason ??
                        (toggleMutation.isPending
                          ? tc('access.operation_pending')
                          : undefined)
                      }
                    >
                      {r.enabled
                        ? t('sso_providers.actions.disable')
                        : t('sso_providers.actions.enable')}
                    </ChromeButton>
                    <ChromeButton
                      variant="ghost"
                      size="sm"
                      onClick={() => openEdit(r)}
                      disabled={manageAccess.disabled}
                      disabledReason={manageAccess.reason}
                    >
                      {t('sso_providers.actions.edit')}
                    </ChromeButton>
                    <ChromeButton
                      variant="ghost"
                      size="sm"
                      onClick={() => setRemoving(r)}
                      disabled={manageAccess.disabled}
                      disabledReason={manageAccess.reason}
                      className="enabled:hover:text-red-soft"
                    >
                      {t('sso_providers.actions.delete')}
                    </ChromeButton>
                  </div>
                ),
              },
            ]}
          />
        )}
      </SectionBody>
    </>
  );
}

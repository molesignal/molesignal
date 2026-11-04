import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog, DataTable } from '@/admin';
import * as emailDomainsApi from '@/api/emailDomains';
import * as instanceApi from '@/api/instance';
import * as ssoApi from '@/api/sso';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { productStateFor } from '@/product/states';
import { IamListPage } from '@/routes/iam/IamLayout';
import { ChromeButton, Pill } from '@/shell/chrome';
import { FormDrawer, FormSubmitFooter } from '@/shell/FormDrawer';
import { queryStateFor } from '@/shell/query/State';
import { SsoPlatformMark } from '@/shell/SsoPlatformMark';
import { toast } from '@/shell/ui/sonner';

import { acceptsPublicAccounts } from './access';
import {
  applySsoCallbackUrls,
  type ProviderDraft,
  draftForChoice,
  draftFromProvider,
  draftIsInvalid,
  draftToInput,
  emptyDraft,
  resolveSsoCallbackUrls,
} from './model';
import { PlatformPicker } from './PlatformPicker';
import type { PlatformChoice } from './presets';
import { ProviderForm } from './ProviderForm';
import { publicAccountsUnrestricted } from './sections/AccessSection';
import { DirectoryTestDialog } from './signinTest/DirectoryTestDialog';
import { singleSignOnProviders } from './signinTest/model';
import { ProviderTestRow, TestStatusWord } from './signinTest/TestStatus';
import { useSigninTestRun } from './signinTest/useSigninTestRun';
import { formatMicros } from '../../rum/_helpers';
import { SigninRequirementRows } from '../SigninRequirements';

export function SsoProviders() {
  const { t } = useTranslation('settings-admin');
  const { t: tc } = useTranslation('common');
  const qc = useQueryClient();
  const manageAccess = useActionAccess({
    permission: 'org.sso.manage',
  });
  const [editing, setEditing] = React.useState<ssoApi.SsoProvider | 'new' | null>(null);
  /** A new provider starts with the platform picker; the form follows the choice. */
  const [picking, setPicking] = React.useState(false);
  const [draft, setDraft] = React.useState<ProviderDraft>(emptyDraft());
  const [removing, setRemoving] = React.useState<ssoApi.SsoProvider | null>(null);
  const testRun = useSigninTestRun();

  // A test may have finished in another tab, and "only allow single sign-on" goes by the answer.
  const q = useQuery({
    queryKey: ['sso', 'providers'],
    queryFn: () => ssoApi.list(),
    refetchOnWindowFocus: true,
  });
  const instanceQuery = useQuery({
    queryKey: ['instance'],
    queryFn: instanceApi.get,
    staleTime: 300_000,
  });
  const callbackUrls = React.useMemo(
    () =>
      resolveSsoCallbackUrls(
        instanceQuery.data?.externalUrl,
        window.location.origin,
      ),
    [instanceQuery.data?.externalUrl],
  );
  const rolesQuery = useQuery({
    queryKey: ['sso', 'provider-roles'],
    queryFn: () => ssoApi.listAssignableRoles(),
    enabled: editing !== null,
  });
  // A provider open to public accounts needs the organization's email domains.
  const domainsQuery = useQuery({
    queryKey: ['iam', 'email-domains'],
    queryFn: () => emailDomainsApi.list(),
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
    setPicking(true);
    setEditing('new');
  };
  const pickPlatform = (choice: PlatformChoice) => {
    const name =
      choice.platform === 'custom'
        ? t(`sso_providers.kinds.${choice.kind}`)
        : t(`sso_providers.platforms.${choice.platform}`);
    setDraft(draftForChoice(choice, name, callbackUrls));
    setPicking(false);
  };
  const openEdit = (provider: ssoApi.SsoProvider) => {
    if (manageAccess.disabled) return;
    setDraft(applySsoCallbackUrls(draftFromProvider(provider), callbackUrls));
    setPicking(false);
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

  const lastProviderToast = (err: unknown) => {
    const e = toApiError(err);
    toast.error(
      e.status === 409 && e.message.includes('its last provider')
        ? t('signin_requirements.last_provider')
        : e.message,
    );
  };

  const removeMutation = useMutation({
    mutationFn: (id: string) => ssoApi.remove(id),
    onSuccess: () => {
      toast.success(t('sso_providers.drawer.toast_deleted'));
      void qc.invalidateQueries({ queryKey: ['sso', 'providers'] });
      setRemoving(null);
    },
    onError: lastProviderToast,
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
    onError: lastProviderToast,
  });

  const submitting = createMutation.isPending || updateMutation.isPending;
  const willBeEnabled = editing === 'new' || (editing !== null && editing.enabled);
  const unrestricted =
    willBeEnabled &&
    publicAccountsUnrestricted(
      draft,
      acceptsPublicAccounts(draft.kind, draft),
      domainsQuery.data,
    );
  const invalid = draftIsInvalid(draft) || unrestricted;
  const dirty =
    editing === 'new' ||
    (editing !== null &&
      JSON.stringify(draftToInput(draft)) !==
        JSON.stringify(draftToInput(draftFromProvider(editing))));
  const controlsDisabled = manageAccess.disabled || submitting;
  // The drawer holds the provider as it was opened; its test may have finished since.
  const saved = editing && editing !== 'new' ? rows.find((row) => row.id === editing.id) ?? editing : null;
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
      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(v) => !v && setRemoving(null)}
        destructive
        title={t('sso_providers.delete_confirm_title')}
        description={t('sso_providers.delete_confirm_description')}
        typeToConfirm={removing?.name}
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
      <DirectoryTestDialog
        providerName={testRun.directory?.name ?? ''}
        open={testRun.directory !== null}
        busy={testRun.testingDirectory}
        onClose={testRun.closeDirectory}
        onSubmit={testRun.submitDirectory}
      />
      <FormDrawer
        open={editing !== null}
        onOpenChange={(v) => !v && closeDrawer()}
        title={
          editing === 'new'
            ? t('sso_providers.drawer.create_title')
            : t('sso_providers.drawer.edit_title', { name: editing?.name ?? '' })
        }
        subtitle={
          (picking
            ? t('sso_providers.picker.subtitle')
            : t('sso_providers.drawer.subtitle')) as string
        }
        footer={
          picking ? (
            <ChromeButton type="button" onClick={closeDrawer}>
              {tc('actions.cancel')}
            </ChromeButton>
          ) : (
            <FormSubmitFooter
              busy={submitting}
              disabled={manageAccess.disabled || !dirty}
              invalid={invalid}
              disabledReason={
                manageAccess.reason ??
                (unrestricted
                  ? t('sso_providers.access.public_reason')
                  : invalid
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
          )
        }
      >
        {picking ? (
          <PlatformPicker onPick={pickPlatform} />
        ) : (
          <form id="sso-provider-form" onSubmit={submitDrawer}>
            <ProviderForm
              draft={draft}
              creating={editing === 'new'}
              testRow={
                saved && (
                  <ProviderTestRow
                    provider={saved}
                    dirty={dirty}
                    waiting={testRun.waitingFor === saved.id}
                    starting={testRun.starting}
                    disabledReason={manageAccess.reason}
                    onTest={() => testRun.begin(saved)}
                  />
                )
              }
              callbackUrls={callbackUrls}
              emailDomains={domainsQuery.data}
              onChangePlatform={() => setPicking(true)}
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
        )}
      </FormDrawer>

      <IamListPage
        title={t('signin_requirements.title')}
        subtitle={t('signin_requirements.subtitle') as string}
      >
        <SigninRequirementRows
          permission="org.signin.manage"
          ssoChoice
          ssoProviders={singleSignOnProviders(rows)}
        />
      </IamListPage>

      <IamListPage
        title={t('sso_providers.title')}
        subtitle={t('sso_providers.subtitle')}
        toolbar={
          <ChromeButton
            variant="primary"
            onClick={openCreate}
            disabled={manageAccess.disabled}
            disabledReason={manageAccess.reason}
          >
            {t('sso_providers.new_provider')}
          </ChromeButton>
        }
        state={pageState}
      >
        <DataTable
          rows={rows}
          rowKey={(r) => r.id}
          columns={[
            {
              key: 'name',
              header: t('sso_providers.columns.name'),
              cell: (r) => (
                <span className="flex min-w-0 items-center gap-2">
                  <SsoPlatformMark platform={r.platform} kind={r.kind} />
                  <span className="truncate font-semibold text-tx-0">{r.name}</span>
                </span>
              ),
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
                        : r.kind === 'oauth2'
                          ? 'orange'
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
              key: 'test',
              header: t('sso_providers.columns.test'),
              cell: (r) => (
                <TestStatusWord
                  provider={r}
                  waiting={testRun.waitingFor === r.id}
                  linked={manageAccess.allowed}
                />
              ),
              width: 130,
            },
            {
              key: 'updated',
              header: t('sso_providers.columns.updated'),
              cell: (r) => (r.updatedAt ? formatMicros(r.updatedAt) : '—'),
              width: 200,
            },
            {
              key: 'actions',
              header: '',
              width: 280,
              cell: (r) => (
                <div
                  className="flex gap-2"
                  onClick={(e) => e.stopPropagation()}
                >
                  <ChromeButton
                    variant="ghost"
                    size="sm"
                    onClick={() => testRun.begin(r)}
                    disabled={manageAccess.disabled || testRun.starting}
                    disabledReason={
                      manageAccess.reason ??
                      (testRun.starting ? tc('access.operation_pending') : undefined)
                    }
                  >
                    {t('sso_providers.actions.test')}
                  </ChromeButton>
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
      </IamListPage>
    </>
  );
}

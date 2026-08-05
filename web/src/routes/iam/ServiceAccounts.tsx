import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Plus, Trash2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { DataTable } from '@/admin';
import * as apiTokens from '@/api/apiTokens';
import * as rolesApi from '@/api/roles';
import { toApiError } from '@/lib/http';
import {
  type ActionAccess,
  useActionAccess,
} from '@/product/actionAccess';
import { productStateFor } from '@/product/states';
import { ChromeButton, IconButton, Pill } from '@/shell/chrome';
import { CopyIconButton } from '@/shell/CopyIconButton';
import {
  FormDrawer,
  FormField,
  FormInput,
  FormSection,
  FormSelect,
  FormSubmitFooter,
} from '@/shell/FormDrawer';
import { queryStateFor } from '@/shell/query/State';
import { toast } from '@/shell/ui/sonner';

import { IamListPage } from './IamLayout';
import { formatMicros } from '../rum/_helpers';

function formatOptionalMicros(value: number | null | undefined, fallback: string): string {
  return value ? formatMicros(value) : fallback;
}

function displayPrefix(token: apiTokens.ApiToken): string {
  return `${token.token_kind === 'rum_client' ? 'msrum' : 'ms'}_${token.prefix}`;
}

export function ServiceAccounts() {
  const { t } = useTranslation('iam');
  const { t: tc } = useTranslation('common');
  const qc = useQueryClient();
  const manageAccess = useActionAccess({
    permission: 'api_tokens.manage',
  });
  const [creating, setCreating] = React.useState(false);

  const q = useQuery({
    queryKey: ['iam', 'api-tokens'],
    queryFn: () => apiTokens.list(),
  });
  const rows = q.data ?? [];
  const queryState = queryStateFor({
    isLoading: q.isLoading,
    isError: q.isError,
    data: rows,
  });
  const pageState = productStateFor(queryState === 'empty' ? null : queryState, {
    error: q.error,
  });

  const revoke = useMutation({
    mutationFn: (token: apiTokens.ApiToken) => apiTokens.revoke(token.id),
    onSuccess: async (_result, token) => {
      toast.success(t('service_accounts.toast_revoked'));
      await Promise.all([
        qc.invalidateQueries({ queryKey: ['iam', 'api-tokens'] }),
        qc.invalidateQueries({
          queryKey:
            token.token_kind === 'rum_client'
              ? ['rum-client-token']
              : ['default-ingestion-token'],
        }),
      ]);
    },
    onError: (err) => toast.error(toApiError(err).message),
  });

  return (
    <>
      <IamListPage
        title={t('service_accounts.title')}
        subtitle={t('service_accounts.subtitle') as string}
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
        state={pageState}
      >
        <DataTable
          rows={rows}
          rowKey={(r) => r.id}
          emptyLabel={t('service_accounts.empty_title')}
          columns={[
            { key: 'name', header: t('service_accounts.columns.name'), cell: (r) => r.name, width: 220 },
            { key: 'prefix', header: t('service_accounts.columns.prefix'), cell: displayPrefix, width: 210 },
            { key: 'role', header: t('service_accounts.columns.role'), cell: (r) => r.role_name, width: 140 },
            {
              key: 'kind',
              header: t('service_accounts.columns.kind'),
              cell: (r) => (
                <span className="text-xs text-tx-2">
                  {t(`service_accounts.kinds.${r.token_kind}`)}
                  {r.application_id ? ` · ${r.application_id}` : ''}
                </span>
              ),
              width: 220,
            },
            {
              key: 'status',
              header: t('service_accounts.columns.status'),
              cell: (r) => (
                <Pill tone={r.revoked ? 'red' : 'green'}>
                  {r.revoked ? t('service_accounts.status_revoked') : t('service_accounts.status_active')}
                </Pill>
              ),
              width: 120,
            },
            {
              key: 'expires',
              header: t('service_accounts.columns.expires'),
              cell: (r) => formatOptionalMicros(r.expires_at_micros, t('service_accounts.never_expires')),
              width: 170,
            },
            {
              key: 'last_used',
              header: t('service_accounts.columns.last_used'),
              cell: (r) => formatOptionalMicros(r.last_used_at_micros, '—'),
              width: 170,
            },
            {
              key: 'actions',
              header: t('service_accounts.columns.actions'),
              width: 88,
              cell: (r) => (
                  <IconButton
                    disabled={
                      manageAccess.disabled || r.revoked || revoke.isPending
                    }
                    disabledReason={
                      manageAccess.reason ??
                      (r.revoked
                        ? t('service_accounts.already_revoked')
                        : revoke.isPending
                          ? tc('access.operation_pending')
                          : undefined)
                    }
                    onClick={(event) => {
                      event.stopPropagation();
                      revoke.mutate(r);
                    }}
                    className="enabled:hover:bg-red-dim enabled:hover:text-red-soft"
                    aria-label={t('service_accounts.revoke_token', { name: r.name })}
                  >
                    <Trash2 className="h-3 w-3" />
                  </IconButton>
                ),
            },
          ]}
        />
      </IamListPage>
      <CreateApiTokenDrawer
        open={creating}
        access={manageAccess}
        onClose={() => setCreating(false)}
      />
    </>
  );
}

function CreateApiTokenDrawer({
  open,
  access,
  onClose,
}: {
  open: boolean;
  access: ActionAccess;
  onClose: () => void;
}) {
  const { t } = useTranslation('iam');
  const { t: tc } = useTranslation('common');
  const qc = useQueryClient();
  const [name, setName] = React.useState('');
  const [roleId, setRoleId] = React.useState('');
  const [expiresInDays, setExpiresInDays] = React.useState('365');
  const [created, setCreated] = React.useState<apiTokens.CreatedApiToken | null>(null);
  const rolesQuery = useQuery({
    queryKey: ['iam', 'roles'],
    queryFn: () => rolesApi.list(),
    enabled: open,
  });

  React.useEffect(() => {
    if (!open) return;
    setName('');
    setRoleId('');
    setExpiresInDays('365');
    setCreated(null);
  }, [open]);

  const create = useMutation({
    mutationFn: () => {
      const trimmedName = name.trim();
      const trimmedDays = expiresInDays.trim();
      const payload: apiTokens.CreateApiTokenPayload = {
        name: trimmedName,
      };
      if (roleId) payload.role_id = roleId;
      if (trimmedDays !== '') {
        payload.expires_in_days = Number(trimmedDays);
      }
      return apiTokens.create(payload);
    },
    onSuccess: async (token) => {
      setCreated(token);
      toast.success(t('service_accounts.toast_created'));
      await qc.invalidateQueries({ queryKey: ['iam', 'api-tokens'] });
    },
    onError: (err) => toast.error(toApiError(err).message),
  });

  const expiresValue =
    expiresInDays.trim() === '' ? null : Number(expiresInDays);
  const invalid =
    name.trim().length === 0 ||
    (expiresValue !== null &&
      (!Number.isFinite(expiresValue) ||
        expiresValue < 1 ||
        expiresValue > 1825));
  const controlsDisabled = access.disabled || create.isPending;
  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    if (access.disabled || invalid || create.isPending) return;
    create.mutate();
  };

  const copyToken = async () => {
    if (!created?.token) return;
    try {
      await navigator.clipboard.writeText(created.token);
      toast.success(t('service_accounts.toast_copied'));
    } catch (err) {
      toast.error(toApiError(err).message);
    }
  };

  return (
    <FormDrawer
      open={open}
      onOpenChange={(next) => !next && onClose()}
      title={t('service_accounts.drawer_title')}
      subtitle={t('service_accounts.drawer_subtitle')}
      footer={
        created ? (
          <ChromeButton variant="primary" onClick={onClose}>
            {t('service_accounts.done')}
          </ChromeButton>
        ) : (
          <FormSubmitFooter
            busy={create.isPending}
            disabled={access.disabled}
            invalid={invalid}
            disabledReason={
              access.reason ??
              (invalid ? tc('access.form_invalid') : undefined)
            }
            onCancel={onClose}
            submitLabel={t('service_accounts.submit_label')}
            formId="api-token-form"
          />
        )
      }
    >
      {created ? (
        <FormSection
          title={t('service_accounts.created_title')}
          description={t('service_accounts.created_description')}
        >
          <div className="flex min-w-0 items-center gap-2 rounded-md border border-bd-0 bg-bg-2 p-2">
            <code className="min-w-0 flex-1 overflow-hidden text-ellipsis whitespace-nowrap font-sans text-xs text-tx-0">
              {created.token}
            </code>
            <CopyIconButton
              type="button"
              onClick={copyToken}
              label={t('service_accounts.copy_token')}
            />
          </div>
        </FormSection>
      ) : (
        <form id="api-token-form" onSubmit={submit}>
          <FormSection>
            <FormField label={t('service_accounts.fields.name')} required>
              <FormInput
                value={name}
                onChange={(event) => setName(event.target.value)}
                placeholder={t('service_accounts.fields.name_placeholder')}
                disabled={controlsDisabled}
                disabledReason={access.reason}
                required
              />
            </FormField>
            <FormField label={t('service_accounts.fields.role')} required>
              <FormSelect
                value={roleId}
                onChange={setRoleId}
                disabled={controlsDisabled}
                disabledReason={access.reason}
                options={[
                  { value: '', label: t('service_accounts.fields.default_role') },
                  ...(rolesQuery.data ?? [])
                    .filter((role) => role.key !== 'rum_client')
                    .map((role) => ({
                      value: role.id,
                      label: role.name,
                    })),
                ]}
              />
            </FormField>
            <FormField
              label={t('service_accounts.fields.expires_in_days')}
              hint={t('service_accounts.fields.expires_hint')}
            >
              <FormInput
                type="number"
                min={1}
                max={1825}
                value={expiresInDays}
                onChange={(event) => setExpiresInDays(event.target.value)}
                disabled={controlsDisabled}
                disabledReason={access.reason}
              />
            </FormField>
          </FormSection>
        </form>
      )}
    </FormDrawer>
  );
}

import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Plus, Trash2, X } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useSearchParams } from 'react-router-dom';

import { DataTable } from '@/admin';
import * as apiTokens from '@/api/apiTokens';
import * as serviceAccountsApi from '@/api/serviceAccounts';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { productStateFor } from '@/product/states';
import { ChromeButton, IconButton, Pill } from '@/shell/chrome';
import { queryStateFor } from '@/shell/query/State';
import { toast } from '@/shell/ui/sonner';

import { IamListPage } from '../IamLayout';
import { CreateApiTokenDrawer } from './CreateApiTokenDrawer';
import { formatMicros } from '../../rum/_helpers';

function formatOptionalMicros(
  value: number | null | undefined,
  fallback: string,
): string {
  return value ? formatMicros(value) : fallback;
}

function displayPrefix(token: apiTokens.ApiToken): string {
  return `${token.token_kind === 'rum_client' ? 'msrum' : 'ms'}_${token.prefix}`;
}

export function ApiTokens() {
  const { t } = useTranslation('iam');
  const { t: tc } = useTranslation('common');
  const queryClient = useQueryClient();
  const [searchParams, setSearchParams] = useSearchParams();
  const manageAccess = useActionAccess({ permission: 'api_tokens.manage' });
  const serviceAccountAccess = useActionAccess({
    permission: 'service_accounts.read',
  });
  const [creating, setCreating] = React.useState(false);
  const serviceAccountId = searchParams.get('service_account_id') ?? '';
  const query = useQuery({
    queryKey: ['iam', 'api-tokens'],
    queryFn: apiTokens.list,
  });
  const serviceAccounts = useQuery({
    queryKey: ['iam', 'service-accounts'],
    queryFn: serviceAccountsApi.list,
    enabled: serviceAccountAccess.allowed,
  });
  const accountNames = new Map(
    (serviceAccounts.data ?? []).map((account) => [account.id, account.name]),
  );
  const rows = (query.data ?? []).filter(
    (token) =>
      !serviceAccountId || token.service_account_id === serviceAccountId,
  );
  const queryState = queryStateFor({
    isLoading: query.isLoading,
    isError: query.isError,
    data: rows,
  });
  const revoke = useMutation({
    mutationFn: (token: apiTokens.ApiToken) => apiTokens.revoke(token.id),
    onSuccess: async (_result, token) => {
      toast.success(t('api_tokens.toast_revoked'));
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ['iam', 'api-tokens'] }),
        queryClient.invalidateQueries({
          queryKey:
            token.token_kind === 'rum_client'
              ? ['rum-client-token']
              : ['default-intake-token'],
        }),
      ]);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const filteredAccountName =
    accountNames.get(serviceAccountId) || serviceAccountId;

  return (
    <>
      <IamListPage
        title={t('api_tokens.title')}
        subtitle={
          serviceAccountId
            ? t('api_tokens.filtered_subtitle', {
                serviceAccount: filteredAccountName,
              })
            : t('api_tokens.subtitle')
        }
        toolbar={
          <div className="flex items-center gap-2">
            {serviceAccountId && (
              <ChromeButton
                onClick={() => {
                  searchParams.delete('service_account_id');
                  setSearchParams(searchParams, { replace: true });
                }}
              >
                <X className="h-3 w-3" />
                {t('api_tokens.clear_filter')}
              </ChromeButton>
            )}
            <ChromeButton
              variant="primary"
              onClick={() => setCreating(true)}
              disabled={manageAccess.disabled}
              disabledReason={manageAccess.reason}
            >
              <Plus className="h-3 w-3" />
              {t('api_tokens.create')}
            </ChromeButton>
          </div>
        }
        state={productStateFor(
          queryState === 'empty' ? null : queryState,
          { error: query.error },
        )}
      >
        <DataTable
          rows={rows}
          rowKey={(token) => token.id}
          emptyLabel={t('api_tokens.empty_title')}
          columns={[
            {
              key: 'name',
              header: t('api_tokens.columns.name'),
              cell: (token) => token.name,
              width: 210,
            },
            {
              key: 'prefix',
              header: t('api_tokens.columns.prefix'),
              cell: displayPrefix,
              width: 205,
            },
            {
              key: 'principal',
              header: t('api_tokens.columns.principal'),
              cell: (token) =>
                token.service_account_id
                  ? accountNames.get(token.service_account_id) ||
                    token.service_account_id
                  : t('api_tokens.current_user'),
              width: 180,
            },
            {
              key: 'role',
              header: t('api_tokens.columns.role'),
              cell: (token) => token.role_name,
              width: 150,
            },
            {
              key: 'kind',
              header: t('api_tokens.columns.kind'),
              cell: (token) => (
                <span className="text-xs text-tx-2">
                  {t(`api_tokens.kinds.${token.token_kind}`)}
                  {token.application_id ? ` · ${token.application_id}` : ''}
                </span>
              ),
              width: 180,
            },
            {
              key: 'status',
              header: t('api_tokens.columns.status'),
              cell: (token) => (
                <Pill tone={token.revoked ? 'red' : 'green'}>
                  {token.revoked
                    ? t('api_tokens.status_revoked')
                    : t('api_tokens.status_active')}
                </Pill>
              ),
              width: 105,
            },
            {
              key: 'expires',
              header: t('api_tokens.columns.expires'),
              cell: (token) =>
                formatOptionalMicros(
                  token.expires_at_micros,
                  t('api_tokens.never_expires'),
                ),
              width: 160,
            },
            {
              key: 'last_used',
              header: t('api_tokens.columns.last_used'),
              cell: (token) =>
                formatOptionalMicros(token.last_used_at_micros, '—'),
              width: 160,
            },
            {
              key: 'actions',
              header: t('api_tokens.columns.actions'),
              width: 80,
              cell: (token) => (
                <IconButton
                  disabled={
                    manageAccess.disabled || token.revoked || revoke.isPending
                  }
                  disabledReason={
                    manageAccess.reason ??
                    (token.revoked
                      ? t('api_tokens.already_revoked')
                      : revoke.isPending
                        ? tc('access.operation_pending')
                        : undefined)
                  }
                  onClick={() => revoke.mutate(token)}
                  className="enabled:hover:bg-red-dim enabled:hover:text-red-soft"
                  aria-label={t('api_tokens.revoke_token', {
                    name: token.name,
                  })}
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
        {...(serviceAccountId ? { defaultServiceAccountId: serviceAccountId } : {})}
        onClose={() => setCreating(false)}
      />
    </>
  );
}

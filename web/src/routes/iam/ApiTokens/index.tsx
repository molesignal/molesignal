import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Plus, X } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useSearchParams } from 'react-router-dom';

import { DataTable, TableExportButton } from '@/admin';
import * as apiTokens from '@/api/apiTokens';
import * as serviceAccountsApi from '@/api/serviceAccounts';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { productStateFor } from '@/product/states';
import { ChromeButton } from '@/shell/chrome';
import { queryStateFor } from '@/shell/query/State';
import { ViewTabs } from '@/shell/tabs/ViewTabs';
import { toast } from '@/shell/ui/sonner';
import { useUsers } from '@/shell/useUsers';

import { IamListPage } from '../IamLayout';
import { TOKEN_TABLE_MIN_WIDTH, useTokenColumns } from './columns';
import { CreateApiTokenDrawer, TOKEN_LIST_KEYS } from './CreateApiTokenDrawer';
import { countRisks, TOKEN_RISKS, tokenRisks, type TokenRisk } from './risk';

type RiskView = 'all' | TokenRisk;

/**
 * Every token of the organization. Tokens for service accounts are issued
 * here; people issue their own tokens in their account settings.
 */
export function ApiTokens() {
  const { t } = useTranslation('iam');
  const queryClient = useQueryClient();
  const [searchParams, setSearchParams] = useSearchParams();
  const revokeAccess = useActionAccess({ permission: 'api_tokens.revoke' });
  const issueAccess = useActionAccess({ permission: 'api_tokens.issue' });
  const serviceAccountAccess = useActionAccess({ permission: 'service_accounts.read' });
  const { nameOf } = useUsers();
  const [creating, setCreating] = React.useState(false);
  const [riskView, setRiskView] = React.useState<RiskView>('all');
  const serviceAccountId = searchParams.get('serviceAccountId') ?? '';
  const query = useQuery({
    queryKey: TOKEN_LIST_KEYS.service_account,
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
    (token) => !serviceAccountId || token.serviceAccountId === serviceAccountId,
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
        queryClient.invalidateQueries({ queryKey: TOKEN_LIST_KEYS.service_account }),
        queryClient.invalidateQueries({ queryKey: TOKEN_LIST_KEYS.me }),
        queryClient.invalidateQueries({
          queryKey:
            token.tokenKind === 'rum_client' ? ['rum-client-token'] : ['default-intake-token'],
        }),
      ]);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const principalName = (token: apiTokens.ApiToken) =>
    token.serviceAccountId
      ? accountNames.get(token.serviceAccountId) || token.serviceAccountId
      : token.applicationId
        ? t('api_tokens.application_principal')
        : nameOf(token.userId);
  const columns = useTokenColumns({
    principal: {
      key: 'principal',
      header: t('api_tokens.columns.principal'),
      value: principalName,
      cell: (token) => <span title={principalName(token)}>{principalName(token)}</span>,
      width: 150,
    },
    revoke: {
      access: revokeAccess,
      pending: revoke.isPending,
      onRevoke: (token) => revoke.mutate(token),
    },
  });
  const filteredAccountName = accountNames.get(serviceAccountId) || serviceAccountId;
  const nowMicros = Date.now() * 1000;
  const riskCounts = countRisks(rows, nowMicros);
  const shownRows =
    riskView === 'all' ? rows : rows.filter((token) => tokenRisks(token, nowMicros).includes(riskView));

  return (
    <>
      <IamListPage
        title={t('api_tokens.title')}
        subtitle={
          serviceAccountId
            ? t('api_tokens.filtered_subtitle', { serviceAccount: filteredAccountName })
            : t('api_tokens.subtitle')
        }
        toolbar={
          <div className="flex items-center gap-2">
            {serviceAccountId && (
              <ChromeButton
                onClick={() => {
                  searchParams.delete('serviceAccountId');
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
              disabled={issueAccess.disabled}
              disabledReason={issueAccess.reason}
            >
              <Plus className="h-3 w-3" />
              {t('api_tokens.create_for_service_account')}
            </ChromeButton>
          </div>
        }
        state={productStateFor(queryState === 'empty' ? null : queryState, {
          error: query.error,
        })}
      >
        <>
          <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
            <ViewTabs<RiskView>
              label={t('api_tokens.risks_label')}
              value={riskView}
              onValueChange={setRiskView}
              items={[
                { key: 'all', label: t('api_tokens.risks.all'), count: rows.length },
                ...TOKEN_RISKS.filter((risk) => riskCounts[risk] > 0 || risk === riskView).map((risk) => ({
                  key: risk,
                  label: t(`api_tokens.risks.${risk}`),
                  count: riskCounts[risk],
                })),
              ]}
            />
            <TableExportButton name="molesignal-api-tokens" columns={columns} rows={shownRows} />
          </div>
          <DataTable
            rows={shownRows}
            rowKey={(token) => token.id}
            emptyLabel={riskView === 'all' ? t('api_tokens.empty_title') : t('api_tokens.no_risky')}
            columns={columns}
            fixedMinWidth={TOKEN_TABLE_MIN_WIDTH.all}
          />
        </>
      </IamListPage>
      <CreateApiTokenDrawer
        open={creating}
        access={issueAccess}
        principal="service_account"
        {...(serviceAccountId ? { defaultServiceAccountId: serviceAccountId } : {})}
        onClose={() => setCreating(false)}
      />
    </>
  );
}

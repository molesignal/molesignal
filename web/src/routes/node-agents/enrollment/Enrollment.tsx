import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import type { TFunction } from 'i18next';
import { Plus } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useSearchParams } from 'react-router-dom';

import { ConfirmDialog, DataTable, type DataTableColumn } from '@/admin';
import * as nodeAgentsApi from '@/api/nodeAgents';
import { type EnrollmentToken, type IssuedEnrollmentToken, nodeAgentKeys } from '@/api/nodeAgents';
import { toApiError } from '@/lib/http';
import { useTimeFormatter } from '@/lib/time';
import { useActionAccess } from '@/product/actionAccess';
import { ProductState } from '@/product/states';
import { ChromeButton } from '@/shell/chrome';
import { toast } from '@/shell/ui/sonner';

import { NodeAgentsPage, useLoadState } from '../Frame';
import { CreateTokenDrawer, IssuedTokenDrawer } from './EnrollmentDrawers';

type TokenState = 'active' | 'used_up' | 'expired' | 'revoked';

function tokenState(token: EnrollmentToken, nowMicros: number): TokenState {
  if (token.revokedAt) return 'revoked';
  if (token.uses >= token.maxUses) return 'used_up';
  if (token.expiresAt <= nowMicros) return 'expired';
  return 'active';
}

/**
 * The tokens new nodes enroll with: what each is for, how much of it is used, and until when it
 * works. A token's secret is shown once, when it is issued.
 */
export function Enrollment() {
  const { t } = useTranslation('infrastructure');
  const time = useTimeFormatter();
  const queryClient = useQueryClient();
  const [params, setParams] = useSearchParams();
  const access = useActionAccess({ permission: 'node_agents.enroll' });
  const [issued, setIssued] = React.useState<IssuedEnrollmentToken>();
  const [revokeTarget, setRevokeTarget] = React.useState<EnrollmentToken>();
  const creating = params.get('create') === '1';
  const now = Date.now() * 1000;

  const tokensQuery = useQuery({
    queryKey: nodeAgentKeys.tokens(),
    queryFn: nodeAgentsApi.listEnrollmentTokens,
  });

  const tokens = tokensQuery.data ?? [];
  const refresh = () => queryClient.invalidateQueries({ queryKey: nodeAgentKeys.tokens() });

  const setCreating = (open: boolean) => {
    const next = new URLSearchParams(params);
    if (open) next.set('create', '1');
    else next.delete('create');
    setParams(next, { replace: true });
  };

  const revoke = useMutation({
    mutationFn: (token: EnrollmentToken) => nodeAgentsApi.revokeEnrollmentToken(token.id),
    onSuccess: async () => {
      setRevokeTarget(undefined);
      toast.success(t('node_agents.enrollment.revoked'));
      await refresh();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const state = useLoadState({
    pending: tokensQuery.isPending,
    error: tokensQuery.error,
    onRetry: () => void refresh(),
  });

  const createAction = (
    <ChromeButton
      variant="primary"
      disabled={access.disabled}
      disabledReason={access.reason}
      onClick={() => setCreating(true)}
    >
      <Plus aria-hidden className="h-3.5 w-3.5" />
      {t('node_agents.enrollment.create')}
    </ChromeButton>
  );

  const none = !state && tokens.length === 0;

  return (
    <>
      <NodeAgentsPage state={state} action={none ? undefined : createAction}>
        {none ? (
          <ProductState
            variant="empty"
            title={t('node_agents.enrollment.empty_title')}
            description={t('node_agents.enrollment.empty_description')}
            action={createAction}
          />
        ) : (
          <div className="overflow-x-auto">
            <DataTable
              rows={tokens}
              columns={tokenColumns(t, (micros) => time.micros(micros, false), now, (token) =>
                access.allowed && tokenState(token, now) === 'active' ? setRevokeTarget : undefined,
              )}
              rowKey={(token) => token.id}
              className="min-w-[860px] rounded-none border-0 bg-transparent"
            />
          </div>
        )}
      </NodeAgentsPage>

      <CreateTokenDrawer
        open={creating}
        onClose={() => setCreating(false)}
        onIssued={(token) => {
          setCreating(false);
          setIssued(token);
          void refresh();
        }}
      />
      <IssuedTokenDrawer token={issued} onClose={() => setIssued(undefined)} />
      <ConfirmDialog
        open={Boolean(revokeTarget)}
        onOpenChange={(open) => !open && setRevokeTarget(undefined)}
        title={t('node_agents.enrollment.revoke_title')}
        description={t('node_agents.enrollment.revoke_description')}
        confirmLabel={t('node_agents.enrollment.revoke')}
        destructive
        busy={revoke.isPending}
        onConfirm={() => revokeTarget && revoke.mutate(revokeTarget)}
      />
    </>
  );
}

function tokenColumns(
  t: TFunction<'infrastructure'>,
  date: (micros: number) => string,
  nowMicros: number,
  revokeOf: (token: EnrollmentToken) => ((token: EnrollmentToken) => void) | undefined,
): DataTableColumn<EnrollmentToken>[] {
  return [
    {
      key: 'description',
      header: t('node_agents.enrollment.description'),
      width: 220,
      cell: (token) => (
        <div className="min-w-0">
          <div className="truncate text-tx-0">{token.description || t('node_agents.enrollment.untitled')}</div>
          <div className="mt-0.5 truncate font-code text-type-micro text-tx-3">
            {Object.entries(token.labels)
              .map(([key, value]) => `${key}=${value}`)
              .join(' ')}
          </div>
        </div>
      ),
    },
    {
      key: 'state',
      header: t('node_agents.columns.status'),
      width: 120,
      cell: (token) => (
        <span className="text-xs text-tx-2">{t(`node_agents.enrollment.state.${tokenState(token, nowMicros)}`)}</span>
      ),
    },
    {
      key: 'uses',
      header: t('node_agents.enrollment.uses'),
      width: 100,
      cell: (token) => (
        <span className="text-xs text-tx-2">
          {token.uses}/{token.maxUses}
        </span>
      ),
    },
    {
      key: 'expires',
      header: t('node_agents.enrollment.expires'),
      width: 170,
      cell: (token) => <span className="text-xs text-tx-2">{date(token.expiresAt)}</span>,
    },
    {
      key: 'created',
      header: t('node_agents.enrollment.created'),
      width: 170,
      cell: (token) => <span className="text-xs text-tx-2">{date(token.createdAt)}</span>,
    },
    {
      key: 'actions',
      header: <span className="sr-only">{t('node_agents.enrollment.revoke')}</span>,
      width: 100,
      cell: (token) => {
        const revoke = revokeOf(token);
        return revoke ? (
          <ChromeButton size="sm" className="text-red" onClick={() => revoke(token)}>
            {t('node_agents.enrollment.revoke')}
          </ChromeButton>
        ) : null;
      },
    },
  ];
}

import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import type { TFunction } from 'i18next';
import { KeyRound, Plus, RotateCw, ShieldOff } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog, DataTable, type DataTableColumn } from '@/admin';
import * as syntheticsApi from '@/api/synthetics';
import type { ProbeAgentToken, ProbeLocation } from '@/api/synthetics';
import { toApiError } from '@/lib/http';
import { ChromeButton } from '@/shell/chrome';
import { toast } from '@/shell/ui/sonner';

import {
  AgentTokenFormDrawer,
  AgentTokenInstructionsDrawer,
} from './TokenDrawers';
import { Section } from '../../components';
import { formatRelativeTimestamp } from '../../model';

export function AgentTokenPanel({
  locations,
  canManage,
}: {
  locations: ProbeLocation[];
  canManage: boolean;
}) {
  const { t, i18n } = useTranslation('synthetics');
  const queryClient = useQueryClient();
  const [createOpen, setCreateOpen] = React.useState(false);
  const [rotateToken, setRotateToken] = React.useState<ProbeAgentToken>();
  const [disableToken, setDisableToken] = React.useState<ProbeAgentToken>();
  const [instructions, setInstructions] = React.useState<
    Awaited<ReturnType<typeof syntheticsApi.createAgentToken>>
  >();
  const query = useQuery({
    queryKey: ['synthetics', 'agent-tokens'],
    queryFn: syntheticsApi.listAgentTokens,
  });
  const refresh = () => queryClient.invalidateQueries({
    queryKey: ['synthetics', 'agent-tokens'],
  });
  const disable = useMutation({
    mutationFn: (tokenId: string) => syntheticsApi.disableAgentToken(tokenId),
    onSuccess: async () => {
      setDisableToken(undefined);
      await refresh();
      toast.success(t('agent_tokens.disabled'));
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const issued = async (
    value: Awaited<ReturnType<typeof syntheticsApi.createAgentToken>>,
  ) => {
    setRotateToken(undefined);
    setInstructions(value);
    await refresh();
  };
  const rows = query.data ?? [];
  return (
    <>
      <Section
        title={t('agent_tokens.title')}
        description={t('agent_tokens.subtitle')}
        action={
          canManage ? (
            <ChromeButton
              size="sm"
              disabled={locations.length === 0}
              disabledReason={t('agents.no_eligible_locations')}
              onClick={() => setCreateOpen(true)}
            >
              <Plus aria-hidden className="h-3.5 w-3.5" />
              {t('agent_tokens.create_action')}
            </ChromeButton>
          ) : undefined
        }
      >
        {query.isError ? (
          <div className="border-t border-bd-0 px-4 py-5 text-sm text-red-soft">
            {toApiError(query.error).message}
          </div>
        ) : (
          <div className="overflow-x-auto border-t border-bd-0">
            <DataTable
              rows={rows}
              columns={columns(t, i18n.language, locations, canManage, {
                rotate: setRotateToken,
                disable: setDisableToken,
              })}
              rowKey={(token) => token.id}
              emptyLabel={query.isPending
                ? t('states.loading')
                : t('agent_tokens.empty')}
              className="min-w-[850px] rounded-none border-0 bg-transparent"
            />
          </div>
        )}
      </Section>
      <AgentTokenFormDrawer
        open={createOpen}
        onOpenChange={setCreateOpen}
        locations={locations}
        onIssued={(value) => void issued(value)}
      />
      <AgentTokenFormDrawer
        open={Boolean(rotateToken)}
        onOpenChange={(open) => !open && setRotateToken(undefined)}
        locations={locations}
        rotateToken={rotateToken}
        onIssued={(value) => void issued(value)}
      />
      <AgentTokenInstructionsDrawer
        instructions={instructions}
        onClose={() => setInstructions(undefined)}
      />
      <ConfirmDialog
        open={Boolean(disableToken)}
        onOpenChange={(open) => !open && setDisableToken(undefined)}
        title={t('agent_tokens.disable_title')}
        description={disableToken
          ? t('agent_tokens.disable_confirm', { name: disableToken.name })
          : undefined}
        confirmLabel={t('agent_tokens.disable_action')}
        destructive
        busy={disable.isPending}
        onConfirm={() => disableToken && disable.mutate(disableToken.id)}
      />
    </>
  );
}

function columns(
  t: TFunction<'synthetics'>,
  locale: string,
  locations: ProbeLocation[],
  canManage: boolean,
  actions: {
    rotate: (token: ProbeAgentToken) => void;
    disable: (token: ProbeAgentToken) => void;
  },
): DataTableColumn<ProbeAgentToken>[] {
  return [
    {
      key: 'name',
      header: t('agent_tokens.name'),
      width: 210,
      cell: (token) => (
        <div className="flex items-center gap-2.5">
          <span className="grid h-8 w-8 place-items-center rounded-md bg-bg-3 text-tx-2">
            <KeyRound aria-hidden className="h-4 w-4" />
          </span>
          <div>
            <div className="font-strong text-tx-0">{token.name}</div>
            <div className="font-code text-type-micro text-tx-3">{token.token_prefix}…</div>
          </div>
        </div>
      ),
    },
    {
      key: 'status',
      header: t('agents.status'),
      width: 100,
      cell: (token) => {
        const expired = token.status === 'active' && isExpired(token);
        return (
          <span
            className={expired
              ? 'text-red-soft'
              : token.status === 'active'
                ? 'text-green-soft'
                : 'text-tx-3'}
          >
            {expired ? t('agent_tokens.expired') : t(`states.${token.status}`)}
          </span>
        );
      },
    },
    {
      key: 'location',
      header: t('agents.location'),
      width: 160,
      cell: (token) => locations.find((location) => location.id === token.location_id)?.name
        ?? token.location_id,
    },
    {
      key: 'last_used',
      header: t('agent_tokens.last_used'),
      width: 130,
      cell: (token) => token.last_used_at
        ? formatRelativeTimestamp(token.last_used_at, locale)
        : t('agent_tokens.never_used'),
    },
    {
      key: 'expires',
      header: t('agent_tokens.expires'),
      width: 130,
      cell: (token) => token.expires_at
        ? formatRelativeTimestamp(token.expires_at, locale)
        : t('agent_tokens.expiry_never'),
    },
    {
      key: 'actions',
      header: t('checks.columns.actions'),
      width: 180,
      cell: (token) => canManage ? (
        <div className="flex items-center gap-1">
          <ChromeButton size="sm" onClick={() => actions.rotate(token)}>
            <RotateCw aria-hidden className="h-3.5 w-3.5" />
            {t('actions.rotate')}
          </ChromeButton>
          <ChromeButton
            size="sm"
            disabled={token.status === 'disabled'}
            onClick={() => actions.disable(token)}
          >
            <ShieldOff aria-hidden className="h-3.5 w-3.5" />
            {t('agent_tokens.disable_action')}
          </ChromeButton>
        </div>
      ) : '—',
    },
  ];
}

function isExpired(token: ProbeAgentToken): boolean {
  return token.expires_at !== undefined && token.expires_at <= Date.now() * 1_000;
}

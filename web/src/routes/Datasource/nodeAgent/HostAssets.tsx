import { useMutation, useQueryClient, type UseQueryResult } from '@tanstack/react-query';
import { ArrowUpRight, CircleCheck, CircleDashed, CircleMinus, Clock, LoaderCircle, PackagePlus } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import * as nodeAgentApi from '@/api/nodeAgent';
import type { NodeAgentAssets } from '@/api/nodeAgent';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { Skeleton } from '@/shell/ui/skeleton';
import { toast } from '@/shell/ui/sonner';

import { AlertTemplates } from './AlertTemplates';
import { type AssetRow, type AssetState, assetRows, hasPending, NODE_AGENT_ASSETS_KEY } from './model';

const STATE_ICON: Record<AssetState, typeof CircleCheck> = {
  ready: CircleCheck,
  pending: CircleDashed,
  waiting: Clock,
  blocked: CircleMinus,
};

const STATE_TEXT: Record<AssetState, string> = {
  ready: 'text-[var(--green-ink)]',
  pending: 'text-tx-3',
  waiting: 'text-tx-3',
  blocked: 'text-[var(--yellow-ink)]',
};

/**
 * What the organization gets for its hosts once the agent sends: the indexes of the host events,
 * the host dashboard, the saved queries and the alert rules. One button installs what is missing;
 * the alert rules come disabled and are switched on one by one below.
 */
export function HostAssets({ query }: { query: UseQueryResult<NodeAgentAssets> }) {
  const { t } = useTranslation('onboarding');
  const queryClient = useQueryClient();
  const access = useActionAccess({ permission: 'dashboards.create' });

  const install = useMutation({
    mutationFn: nodeAgentApi.install,
    onSuccess: (assets) => {
      queryClient.setQueryData(NODE_AGENT_ASSETS_KEY, assets);
      void queryClient.invalidateQueries({ queryKey: ['alerts', 'rules'] });

      const skipped = assets.skipped ?? [];

      if (skipped.length > 0) {
        toast.warning(
          t('datasource_page.node_agent.assets.skipped', {
            items: skipped.map((key) => t(`datasource_page.node_agent.assets.${key}.title`)).join(', '),
          }),
        );
      } else {
        toast.success(t('datasource_page.node_agent.assets.installed'));
      }
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  if (query.isPending) {
    return (
      <div className="space-y-2" aria-hidden>
        <Skeleton className="h-4 w-72" />
        <Skeleton className="h-4 w-64" />
        <Skeleton className="h-4 w-80" />
      </div>
    );
  }

  if (query.isError || !query.data) {
    return (
      <p className="m-0 font-sans text-sm text-tx-2">
        {t('datasource_page.node_agent.assets.unreadable', { message: toApiError(query.error).message })}
      </p>
    );
  }

  const assets = query.data;
  const rows = assetRows(assets);
  const pending = hasPending(rows);

  return (
    <div data-node-agent-assets="" className="space-y-[22px]">
      <div className="min-w-0">
        <ul className="m-0 flex list-none flex-col gap-[10px] p-0">
          {rows.map((row) => (
            <AssetLine key={row.key} row={row} />
          ))}
        </ul>
        <div className="mt-[14px] flex flex-wrap items-center gap-2">
          <ChromeButton
            variant={pending ? 'primary' : 'default'}
            disabled={!pending || install.isPending || access.disabled}
            disabledReason={access.reason}
            onClick={() => install.mutate()}
            data-node-agent-install=""
          >
            {install.isPending ? (
              <LoaderCircle aria-hidden className="h-3.5 w-3.5 animate-spin" />
            ) : (
              <PackagePlus aria-hidden className="h-3.5 w-3.5" />
            )}
            {t(pending ? 'datasource_page.node_agent.assets.install' : 'datasource_page.node_agent.assets.up_to_date')}
          </ChromeButton>
          <span className="font-sans text-xs text-tx-3">{t('datasource_page.node_agent.assets.install_hint')}</span>
        </div>
      </div>

      {assets.alerts && <AlertTemplates alerts={assets.alerts} />}
    </div>
  );
}

function AssetLine({ row }: { row: AssetRow }) {
  const { t } = useTranslation('onboarding');
  const Icon = STATE_ICON[row.state];
  const base = `datasource_page.node_agent.assets.${row.key}`;

  return (
    <li data-node-agent-asset={row.key} data-state={row.state} className="flex min-w-0 items-start gap-[10px]">
      <Icon aria-hidden className={cn('mt-[3px] h-3.5 w-3.5 shrink-0', STATE_TEXT[row.state])} />
      <div className="min-w-0 flex-1 font-sans text-sm leading-relaxed">
        <span className="font-strong text-tx-0">{t(`${base}.title`)}</span>
        <span className="text-tx-2"> · {t(`${base}.${row.detail}`, row.values)}</span>
        {row.href && (
          <Link
            to={row.href}
            className="ml-2 inline-flex items-center gap-0.5 text-tx-2 outline-none transition-colors hover:text-tx-0 focus-visible:text-tx-0 focus-visible:underline"
          >
            {t('datasource_page.node_agent.assets.open')}
            <ArrowUpRight aria-hidden className="h-3 w-3" />
          </Link>
        )}
      </div>
    </li>
  );
}

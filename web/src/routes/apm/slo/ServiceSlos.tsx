import { useQuery } from '@tanstack/react-query';
import { Plus, Target } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { sloApi, sloQueryKeys } from '@/api/apmSlo';
import { useActionAccess } from '@/product/actionAccess';
import { cn } from '@/shell/lib/cn';

import { formatBudget, formatSli, SLO_STATE_TEXT } from './model';
import { SloFormDrawer } from './SloForm';
import { ApmCard } from '../frame/Page';
import type { ServiceAddress } from '../service/useService';

/** The objectives of a service on its overview: how each stands, and a way to set another. */
export function ServiceSlos({ address }: { address: ServiceAddress }) {
  const { t } = useTranslation('apm');
  const access = useActionAccess({ permission: 'apm.slos.manage' });
  const [open, setOpen] = React.useState(false);
  const filter = { namespace: address.namespace, service: address.name, environment: address.environment };
  const statuses = useQuery({
    queryKey: sloQueryKeys.list(address.orgId, filter),
    queryFn: () => sloApi.list(filter),
    enabled: Boolean(address.orgId),
    refetchInterval: 60_000,
  });
  const items = statuses.data ?? [];

  return (
    <ApmCard
      icon={Target}
      tone="text-[var(--green-ink)]"
      title={t('slo.service_title')}
      testId="service-slos"
      actions={
        !access.disabled && (
          <button
            type="button"
            onClick={() => setOpen(true)}
            className="inline-flex items-center gap-1 font-sans text-xs font-strong text-tx-2 hover:text-tx-0 focus-visible:text-tx-0 focus-visible:outline-none"
          >
            <Plus aria-hidden className="h-3.5 w-3.5" />
            {t('slo.new')}
          </button>
        )
      }
    >
      {items.length === 0 ? (
        <p className="px-3 py-6 text-center font-sans text-xs text-tx-3">{statuses.isPending ? '…' : t('slo.service_empty')}</p>
      ) : (
        items.map((status) => (
          <Link
            key={status.slo.id}
            to={`/apm/slos?slo=${encodeURIComponent(status.slo.id)}`}
            data-apm-service-slo={status.slo.id}
            className="grid h-8 grid-cols-[64px_minmax(0,1fr)_120px_128px] items-center gap-3 border-b border-bd-0 px-3 font-sans text-xs last:border-b-0 hover:bg-bg-2 focus-visible:bg-bg-2 focus-visible:outline-none"
          >
            <span className={cn('truncate font-strong', SLO_STATE_TEXT[status.state])}>{t(`slo.state.${status.state}`)}</span>
            <span className="truncate text-tx-0">{status.slo.name}</span>
            <span className="text-right tabular-nums text-tx-1">
              {formatSli(status.sli)} <span className="text-tx-3">/ {formatSli(status.slo.objective)}</span>
            </span>
            <span className="truncate text-right tabular-nums text-tx-2">
              {t('slo.budget_left', { value: formatBudget(status.budgetRemaining) })}
            </span>
          </Link>
        ))
      )}
      <SloFormDrawer
        open={open}
        target={{ slo: null, service: { namespace: address.namespace, name: address.name, environment: address.environment } }}
        onClose={() => setOpen(false)}
      />
    </ApmCard>
  );
}

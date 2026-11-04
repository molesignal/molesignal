import { useQuery } from '@tanstack/react-query';
import { Plus, Search } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { type ServiceLevelObjective, sloApi, sloQueryKeys, type SloState } from '@/api/apmSlo';
import { useActionAccess } from '@/product/actionAccess';
import { cn } from '@/shell/lib/cn';
import { Button } from '@/shell/ui/button';
import { FittedSelect } from '@/shell/ui/fitted-select';
import { useAuthStore } from '@/stores/auth';

import { SLO_STATE_ORDER, SLO_STATE_TEXT } from './model';
import { SloDrawer } from './SloDrawer';
import { SloFormDrawer, type SloFormTarget } from './SloForm';
import { SloTable } from './SloTable';
import { ApmNavigation } from '../frame/Navigation';
import { ApmCard, ApmPage, ApmQueryState } from '../frame/Page';
import { useAddressedDrawer } from '../frame/useApmScope';

const ALL = '__all__';

/**
 * Every service level objective of the organization, the ones in trouble
 * first: what each promises, what it met and how fast it spends its budget.
 * An objective opens in a drawer; whoever manages APM sets new ones.
 */
export function ApmSlos() {
  const { t } = useTranslation('apm');
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');
  const access = useActionAccess({ permission: 'apm.slos.manage' });
  const drawer = useAddressedDrawer('slo');
  const [search, setSearch] = React.useState('');
  const [state, setState] = React.useState<SloState | ''>('');
  const [form, setForm] = React.useState<{ open: boolean; target: SloFormTarget | null }>({ open: false, target: null });

  const statuses = useQuery({
    queryKey: sloQueryKeys.list(orgId),
    queryFn: () => sloApi.list(),
    enabled: Boolean(orgId),
    refetchInterval: 60_000,
  });

  const all = statuses.data ?? [];
  const counts = Object.fromEntries(SLO_STATE_ORDER.map((value) => [value, all.filter((item) => item.state === value).length])) as Record<SloState, number>;
  const needle = search.trim().toLocaleLowerCase();
  const rows = all
    .filter((item) => !state || item.state === state)
    .filter((item) => !needle || `${item.slo.name} ${item.slo.service} ${item.slo.transaction?.name ?? ''}`.toLocaleLowerCase().includes(needle))
    .sort((left, right) => SLO_STATE_ORDER.indexOf(left.state) - SLO_STATE_ORDER.indexOf(right.state) || (left.budgetRemaining ?? 2) - (right.budgetRemaining ?? 2));
  const edit = (slo: ServiceLevelObjective) => setForm({ open: true, target: { slo } });

  return (
    <ApmPage page="slos" title={t('title')} subtitle={t('subtitle')} breadcrumbs={null} navigation={<ApmNavigation />}>
      <div data-apm-slo-summary="" className="flex flex-wrap items-center gap-x-4 gap-y-1 px-1 font-sans text-xs text-tx-2">
        <span className="font-strong text-tx-0">{t('slo.summary', { count: all.length })}</span>
        {SLO_STATE_ORDER.map((value) => (
          <span key={value} className={cn('font-strong', counts[value] === 0 ? 'text-tx-3' : SLO_STATE_TEXT[value])}>
            {t(`slo.state.${value}`)} <span className="tabular-nums">{counts[value]}</span>
          </span>
        ))}
      </div>
      <ApmCard
        testId="slos"
        title={
          <span className="flex items-center gap-2">
            <label className="relative">
              <span className="sr-only">{t('slo.search')}</span>
              <Search aria-hidden className="pointer-events-none absolute left-2.5 top-2 h-3.5 w-3.5 text-tx-3" />
              <input
                value={search}
                onChange={(event) => setSearch(event.target.value)}
                placeholder={t('slo.search')}
                className="h-8 w-56 rounded-md border-0 bg-[var(--control-surface)] pl-8 pr-2.5 font-sans text-xs font-body text-tx-1 outline-none placeholder:text-tx-3 hover:bg-bg-3 focus-visible:bg-bg-3"
              />
            </label>
            <FittedSelect
              aria-label={t('slo.columns.state')}
              value={state || ALL}
              onValueChange={(value) => setState(value === ALL ? '' : (value as SloState))}
              options={[{ value: ALL, label: t('slo.all_states') }, ...SLO_STATE_ORDER.map((value) => ({ value, label: t(`slo.state.${value}`) }))]}
              maxWidth={160}
              className="h-8 border-0 bg-[var(--control-surface)] px-2.5 text-xs font-strong text-tx-1 hover:bg-bg-3"
            />
          </span>
        }
        actions={
          !access.disabled && (
            <Button variant="outline" size="sm" onClick={() => setForm({ open: true, target: null })} data-apm-slo-new="">
              <Plus aria-hidden className="h-3.5 w-3.5" />
              {t('slo.new')}
            </Button>
          )
        }
      >
        <ApmQueryState
          query={statuses}
          empty={rows.length === 0}
          emptyTitle={all.length === 0 ? t('slo.empty') : t('slo.filtered_empty')}
          emptyDescription={all.length === 0 ? t('slo.empty_description') : undefined}
        >
          <SloTable statuses={rows} onOpen={(id) => drawer.open(id)} />
        </ApmQueryState>
      </ApmCard>
      <SloDrawer id={drawer.value} onClose={() => drawer.close()} onEdit={edit} />
      <SloFormDrawer open={form.open} target={form.target} onClose={() => setForm((current) => ({ ...current, open: false }))} />
    </ApmPage>
  );
}

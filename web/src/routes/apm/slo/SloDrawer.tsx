import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Flame, Pencil, Trash2 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog } from '@/admin';
import * as alertsApi from '@/api/alerts';
import { type ServiceLevelObjective, sloApi, sloQueryKeys } from '@/api/apmSlo';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { cn } from '@/shell/lib/cn';
import { Button } from '@/shell/ui/button';
import { toast } from '@/shell/ui/sonner';
import { useAuthStore } from '@/stores/auth';

import { formatCount } from '../format';
import { burnRateRules, formatBudget, formatBurn, formatSli, SLO_STATE_TEXT, sloTarget } from './model';
import { SloAlertsCard, useSloAlertRules } from './SloAlerts';
import { SloDays } from './SloDays';
import { ApmDrawer } from '../frame/ApmDrawer';
import { ApmCard, ApmQueryState } from '../frame/Page';

/** Each window of the burn rate, the rate past which it is marked, and in which ink: past the
 * first two the objective is at risk; past 1 over a day the budget runs out before the window ends. */
const BURN_WINDOWS = [
  ['one_hour', 'oneHour', 14.4, 'text-[var(--red-ink)]'],
  ['six_hours', 'sixHours', 6, 'text-[var(--red-ink)]'],
  ['one_day', 'oneDay', 1, 'text-[var(--yellow-ink)]'],
] as const;

/**
 * An objective opened over its list: how it stands and why, whether each day
 * of its window met the objective, how fast it spends its budget
 * over an hour, six hours and a day, and the burn-rate alerts that watch it.
 * Deleting it deletes those alerts first, since they cannot outlive it.
 */
export function SloDrawer({
  id,
  onClose,
  onEdit,
}: {
  id: string | null;
  onClose: () => void;
  onEdit: (slo: ServiceLevelObjective) => void;
}) {
  const { t } = useTranslation('apm');
  const orgId = useAuthStore((state) => state.ctx?.orgId ?? '');
  const queryClient = useQueryClient();
  const manage = useActionAccess({ permission: 'apm.slos.manage' });
  const canDelete = useActionAccess({ permission: 'apm.slos.delete' });
  const alerting = useActionAccess({ permission: 'alerts.rules.manage' });
  const detail = useQuery({
    queryKey: sloQueryKeys.detail(orgId, id ?? ''),
    queryFn: () => sloApi.get(id ?? ''),
    enabled: Boolean(orgId && id),
    refetchInterval: 60_000,
  });
  const status = detail.data?.status;
  const slo = status?.slo;
  const watching = useSloAlertRules(id);
  const [removing, setRemoving] = React.useState(false);

  const remove = useMutation({
    mutationFn: async () => {
      for (const rule of watching.rules) await alertsApi.remove(rule.id);
      await sloApi.remove(id ?? '');
    },
    onSuccess: async () => {
      await Promise.all([queryClient.invalidateQueries({ queryKey: sloQueryKeys.all }), queryClient.invalidateQueries({ queryKey: ['alerts', 'rules'] })]);
      toast.success(t('slo.removed'));
      setRemoving(false);
      onClose();
    },
    onError: (error) => {
      const failure = toApiError(error);
      // Rules this reader cannot see still watch the objective.
      toast.error(failure.status === 409 ? t('slo.remove_watched') : failure.message);
    },
  });
  const alert = useMutation({
    mutationFn: async () => {
      if (!slo) return [];
      const rules = burnRateRules(slo, {
        fast: t('slo.alerts.fast_name', { name: slo.name }),
        slow: t('slo.alerts.slow_name', { name: slo.name }),
        description: t('slo.alerts.description', { name: slo.name }),
      });
      return Promise.all(rules.map((rule) => alertsApi.create(rule)));
    },
    onSuccess: async (created) => {
      await queryClient.invalidateQueries({ queryKey: ['alerts', 'rules'] });
      toast.success(t('slo.alerts.created', { count: created.length }));
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  return (
    <ApmDrawer
      open={Boolean(id)}
      onClose={onClose}
      testId="slo"
      title={slo?.name ?? t('slo.title')}
      subtitle={slo ? `${slo.service} · ${slo.environment}${slo.transaction ? ` · ${slo.transaction.name}` : ''}` : undefined}
      actions={
        slo && !manage.disabled ? (
          <>
            <Button variant="ghost" size="icon" className="h-8 w-8" aria-label={t('slo.edit')} title={t('slo.edit')} onClick={() => onEdit(slo)}>
              <Pencil className="h-4 w-4" />
            </Button>
            {!canDelete.disabled && (
            <Button variant="ghost" size="icon" className="h-8 w-8" aria-label={t('slo.remove')} title={t('slo.remove')} onClick={() => setRemoving(true)}>
              <Trash2 className="h-4 w-4" />
            </Button>
            )}
          </>
        ) : undefined
      }
    >
      <ApmQueryState query={detail} empty={false}>
        {status && slo && detail.data && (
          <>
            <div data-apm-slo-facts="" className="flex flex-wrap items-baseline gap-x-5 gap-y-1 font-sans text-xs text-tx-2">
              <span className={cn('text-sm font-strong', SLO_STATE_TEXT[status.state])}>{t(`slo.state.${status.state}`)}</span>
              <span>
                {t('slo.facts.sli')} <span className="font-strong tabular-nums text-tx-0">{formatSli(status.sli)}</span>
                <span className="text-tx-3"> / {formatSli(slo.objective)}</span>
              </span>
              <span>
                {t('slo.facts.budget')} <span className="font-strong tabular-nums text-tx-0">{formatBudget(status.budgetRemaining)}</span>
              </span>
              <span>{sloTarget(slo, t)}</span>
              <span>{t('slo.window', { count: slo.windowDays })}</span>
              <span>
                {t('slo.facts.requests', { good: formatCount(Math.round(status.goodRequests)), total: formatCount(status.totalRequests) })}
              </span>
            </div>
            <ApmCard icon={Flame} tone="text-[var(--orange-ink)]" title={t('slo.burn.title')} description={t('slo.burn.description')} testId="slo-burn">
              <div className="grid grid-cols-3 gap-px bg-bd-0">
                {BURN_WINDOWS.map(([window, field, line, ink]) => {
                  const rate = status.burnRates[field];
                  const hot = rate !== null && rate !== undefined && rate > line;
                  return (
                    <div key={window} data-apm-slo-burn={window} className="bg-[var(--functional-surface)] px-3 py-2 font-sans text-xs">
                      <div className="text-tx-2">{t(`slo.burn.${window}`)}</div>
                      <div className={cn('mt-0.5 text-base font-strong tabular-nums', hot ? ink : 'text-tx-0')}>{formatBurn(rate)}</div>
                    </div>
                  );
                })}
              </div>
            </ApmCard>
            {watching.readable && watching.loaded && (
              <SloAlertsCard rules={watching.rules} canCreate={alerting.allowed} creating={alert.isPending} onCreate={() => alert.mutate()} />
            )}
            <SloDays slo={slo} history={detail.data.history} now={detail.dataUpdatedAt * 1_000} />
          </>
        )}
      </ApmQueryState>
      <ConfirmDialog
        open={removing}
        onOpenChange={setRemoving}
        destructive
        title={t('slo.remove_title', { name: slo?.name ?? '' })}
        description={watching.rules.length > 0 ? t('slo.remove_with_alerts', { count: watching.rules.length }) : t('slo.remove_description')}
        confirmLabel={t('slo.remove')}
        cancelLabel={t('actions.cancel')}
        busy={remove.isPending}
        busyLabel={t('slo.removing')}
        disabled={watching.rules.length > 0 && alerting.disabled}
        disabledReason={alerting.reason}
        onConfirm={() => remove.mutate()}
      />
    </ApmDrawer>
  );
}

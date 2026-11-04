import { useQuery } from '@tanstack/react-query';
import { BellPlus, BellRing } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import * as alertsApi from '@/api/alerts';
import { useActionAccess } from '@/product/actionAccess';
import { cn } from '@/shell/lib/cn';
import { Button } from '@/shell/ui/button';
import type { AlertRule } from '@/types/alerting';

import { SEVERITY_TEXT } from '../frame/alerts';
import { ApmCard } from '../frame/Page';

/** The alert rules that watch an objective's burn rate, for those who may read rules. */
export function useSloAlertRules(sloId: string | null) {
  const reader = useActionAccess({ permission: 'alerts.rules.read' });
  const rules = useQuery({
    queryKey: ['alerts', 'rules'],
    queryFn: () => alertsApi.list(),
    enabled: Boolean(sloId) && reader.allowed,
  });

  return {
    readable: reader.allowed,
    loaded: rules.isSuccess,
    rules: (rules.data ?? []).filter((rule) => rule.query.apm?.sloId === sloId),
  };
}

/**
 * The burn-rate alerts on an objective: each rule with the line it pages on
 * and how bad that is, or, while none watches it, the button that sets the
 * pair the SRE workbook recommends.
 */
export function SloAlertsCard({
  rules,
  canCreate,
  creating,
  onCreate,
}: {
  rules: readonly AlertRule[];
  canCreate: boolean;
  creating: boolean;
  onCreate: () => void;
}) {
  const { t } = useTranslation('apm');
  const { t: ta } = useTranslation('alerts');

  return (
    <ApmCard icon={BellRing} tone="text-[var(--red-ink)]" title={t('slo.alerts.title')} description={t('slo.alerts.card_description')} testId="slo-alerts">
      {rules.length === 0 ? (
        <div className="flex min-h-10 items-center justify-between gap-3 px-3 py-1.5 font-sans text-xs text-tx-2">
          <span>{t('slo.alerts.none')}</span>
          {canCreate && (
            <Button variant="ghost" size="sm" onClick={onCreate} disabled={creating} data-apm-slo-alerts="">
              <BellPlus aria-hidden className="h-3.5 w-3.5" />
              {t('slo.alerts.create')}
            </Button>
          )}
        </div>
      ) : (
        <ul className="divide-y divide-bd-0">
          {rules.map((rule) => {
            const tier = rule.thresholds?.[0];
            const severity = tier?.severity ?? rule.severity ?? 'warning';
            return (
              <li key={rule.id} data-apm-slo-rule={rule.id} className="flex min-h-9 items-center gap-3 px-3 py-1.5 font-sans text-xs">
                <Link
                  to={`/alerts/rules/${encodeURIComponent(rule.id)}/edit`}
                  className="min-w-0 flex-1 truncate font-strong text-tx-0 hover:underline focus-visible:underline focus-visible:outline-none"
                >
                  {rule.name}
                </Link>
                <span className="shrink-0 text-tx-2">
                  {t('slo.alerts.rule', { threshold: tier?.threshold ?? rule.trigger.threshold, window: windowLabel(rule.query.periodSecs, t) })}
                </span>
                <span className={cn('w-14 shrink-0 text-right font-strong', rule.enabled ? SEVERITY_TEXT[severity] : 'text-tx-3')}>
                  {rule.enabled ? ta(`severity.${severity}`) : t('slo.alerts.off')}
                </span>
              </li>
            );
          })}
        </ul>
      )}
    </ApmCard>
  );
}

function windowLabel(seconds: number, t: (key: string, options: { count: number }) => string): string {
  return seconds % 3_600 === 0
    ? t('slo.alerts.window_hours', { count: seconds / 3_600 })
    : t('slo.alerts.window_minutes', { count: Math.round(seconds / 60) });
}

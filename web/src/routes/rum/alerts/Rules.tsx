import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { rumNewRuleHref } from '@/routes/alerts/rule-workbench/rumSource';
import { cn } from '@/shell/lib/cn';
import type { AlertRule } from '@/types/alerting';

import { TEXT_LINK } from './GoingOn';
import { type RuleStatus, ruleStatus } from './model';
import { Section } from '../performance/PanelSection';

/** The ink of how a rule stands: a rule that is fine is green, one that fires is red, one that is on its way yellow, and one without data orange. */
const STATUS_INK: Record<RuleStatus, string> = {
  firing: 'text-[var(--red-ink)]',
  dropped: 'text-[var(--purple-ink)]',
  no_data: 'text-[var(--orange-ink)]',
  pending: 'text-[var(--yellow-ink)]',
  healthy: 'text-[var(--green-ink)]',
  off: 'text-tx-3',
};

/**
 * The rules that watch the application, in the environment in front or in every one: the name, the
 * figure, how the rule stands. Those who may manage rules edit them, or make another in the rule
 * editor with the application and the environment filled in.
 */
export function RulesOfApplication({
  rules,
  application,
  environment,
  canManage,
  pending,
  failed,
  onRetry,
}: {
  rules: readonly AlertRule[];
  application: string;
  environment: string;
  canManage: boolean;
  pending: boolean;
  failed: boolean;
  onRetry: () => void;
}) {
  const { t } = useTranslation('rum');
  const { t: ta } = useTranslation('alerts');

  return (
    <Section
      title={t('alerts.rules.title')}
      count={pending || failed ? undefined : rules.length}
      action={
        canManage ? (
          <Link to={rumNewRuleHref({ application, environment })} data-rum-alert-new-rule="" className={TEXT_LINK}>
            {t('alerts.rules.new')}
          </Link>
        ) : undefined
      }
      pending={pending}
      failed={failed}
      onRetry={onRetry}
      empty={rules.length === 0}
      emptyText={t('alerts.rules.empty')}
      testId="alerts-rules"
    >
      <ul data-rum-alert-rules="" className="m-0 flex list-none flex-col p-0">
        {rules.map((rule) => {
          const status = ruleStatus(rule);

          return (
            <li
              key={rule.id}
              data-rum-alert-rule={rule.id}
              className="grid min-h-8 grid-cols-[minmax(0,1fr)_auto_3.5rem_2rem] items-center gap-x-2.5 border-t border-bd-0 py-1.5 text-xs first:border-t-0 first:pt-0"
            >
              <span className="truncate font-strong text-tx-0" title={rule.name}>
                {rule.name}
              </span>
              <span className="truncate text-tx-3">{rule.query.rum ? ta(`workbench.rum.metrics.${rule.query.rum.metric}`) : ''}</span>
              <span data-rum-alert-status={status} className={cn('font-strong', STATUS_INK[status])}>
                {t(`alerts.rules.status.${status}`)}
              </span>
              {canManage ? (
                <Link to={`/alerts/rules/${encodeURIComponent(rule.id)}/edit`} className={cn(TEXT_LINK, 'text-right')}>
                  {t('alerts.rules.edit')}
                </Link>
              ) : (
                <span />
              )}
            </li>
          );
        })}
      </ul>
    </Section>
  );
}

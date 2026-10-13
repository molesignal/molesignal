import { ArrowRight, Repeat } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { EscalationPolicy } from '@/types/alerting';

import { type TargetNames, secsLabel, stepTargetNames } from './steps';

/**
 * A policy as the path an unacknowledged incident takes: who is told first,
 * how long they have, who is told next, and whether it all starts again.
 *
 * It is what a list of policies is read for. A count of steps says a policy
 * has three of them; the chain says who is woken, and when.
 */
export function EscalationChain({ policy, names }: { policy: EscalationPolicy; names: TargetNames }) {
  const { t } = useTranslation('alerts');

  return (
    <ol data-escalation-chain className="flex min-w-0 flex-wrap items-center gap-x-[6px] gap-y-[6px]">
      {policy.steps.map((step, index) => {
        const targets = stepTargetNames(step, names);
        const previous = policy.steps[index - 1];

        return (
          <li key={index} className="flex min-w-0 items-center gap-[6px]">
            {previous && (
              // How long the step before had before the incident moved on.
              <span className="inline-flex shrink-0 items-center gap-[4px] text-xs tabular-nums text-tx-2">
                {secsLabel(previous.ack_timeout_secs)}
                <ArrowRight aria-hidden="true" className="h-3 w-3 text-tx-3" />
              </span>
            )}
            <span className="inline-flex min-w-0 items-center gap-[6px] rounded-md bg-bg-2 px-[8px] py-[3px] font-sans text-xs">
              <span aria-hidden="true" className="font-mono tabular-nums text-tx-3">
                {index + 1}
              </span>
              <span className="truncate text-tx-0">
                {targets.length > 0 ? targets.join(', ') : t('escalations.no_target')}
              </span>
              {step.min_severity && (
                <span className="shrink-0 text-tx-2">
                  {t('escalations.from_severity', { severity: t(`severity.${step.min_severity}`) })}
                </span>
              )}
            </span>
          </li>
        );
      })}
      {policy.repeat && (
        <li className="inline-flex shrink-0 items-center gap-[4px] text-xs tabular-nums text-tx-2">
          <Repeat aria-hidden="true" className="h-3 w-3 text-tx-3" />
          {t('escalations.repeats', { count: policy.max_loops })}
        </li>
      )}
    </ol>
  );
}

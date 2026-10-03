import { useTranslation } from 'react-i18next';

import { Dot, Pill } from '@/shell/chrome';

import { CONDITION_TONE, type StreamCondition } from './streamCondition';

/**
 * Stream condition as a filled pill. When every visible row shares one
 * condition (`quiet`), the fill is dropped: the page already says it once, so
 * a column of identical red pills would only add noise.
 */
export function ConditionBadge({
  condition,
  quiet = false,
}: {
  condition: StreamCondition;
  quiet?: boolean;
}) {
  const { t } = useTranslation('onboarding');
  const tone = CONDITION_TONE[condition];
  const label = t(`home.condition.${condition}`);

  if (quiet) {
    return (
      <span className="inline-flex items-center gap-1.5 whitespace-nowrap font-sans text-xs font-medium text-tx-1">
        <Dot tone={tone} />
        {label}
      </span>
    );
  }
  return (
    <Pill tone={tone}>
      <Dot tone={tone} />
      {label}
    </Pill>
  );
}

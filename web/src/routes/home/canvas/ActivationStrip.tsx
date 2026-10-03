import { ChevronRight, Sparkles } from 'lucide-react';
import type * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { ActivationState } from '@/product/activation';
import { cn } from '@/shell/lib/cn';

/**
 * The setup nudge. It sits directly under the verdict, not at the foot of the
 * page: when setup is unfinished, "what next" is the first thing a new
 * operator needs, and it disappears once every step is done.
 */
export function ActivationStrip({
  state,
  onOpen,
  riseIndex,
}: {
  state: ActivationState;
  onOpen: () => void;
  riseIndex?: number;
}) {
  const { t } = useTranslation('onboarding');
  const remainingCount = state.totalCount - state.completedCount;
  const nextStep = state.steps.find((step) => !step.completed);

  if (remainingCount <= 0) return null;

  return (
    <div
      role="status"
      data-home-surface="activation"
      style={
        riseIndex !== undefined
          ? ({ '--rise-index': riseIndex } as React.CSSProperties)
          : undefined
      }
      className={cn(
        'flex min-h-[48px] flex-wrap items-center gap-x-3 gap-y-1 rounded-md bg-[var(--functional-surface)] px-[20px] py-1.5 [box-shadow:inset_3px_0_0_var(--indigo),var(--shadow-functional-surface)]',
        riseIndex !== undefined && 'home-rise',
      )}
    >
      <Sparkles aria-hidden="true" className="h-4 w-4 shrink-0 text-indigo-soft" />
      <div className="flex min-w-0 flex-1 flex-wrap items-center gap-x-2 gap-y-1">
        <span className="font-sans text-sm font-strong text-tx-0">
          {remainingCount === 1
            ? t('activation.remaining_single')
            : t('activation.remaining_multiple', { count: remainingCount })}
        </span>
        {nextStep && (
          <>
            <span aria-hidden="true" className="text-tx-3">
              ·
            </span>
            <span className="font-sans text-sm text-tx-2">{t('activation.next')}</span>
            <span className="truncate font-sans text-sm font-strong text-tx-1">
              {t(`activation.${nextStep.labelKey}`)}
            </span>
          </>
        )}
      </div>
      <button
        type="button"
        onClick={onOpen}
        className="inline-flex h-[32px] items-center gap-1 rounded px-2 font-sans text-xs font-strong text-indigo-soft transition-colors duration-fast hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0 focus-visible:outline-none"
      >
        {t('activation.complete_action')}
        <ChevronRight aria-hidden="true" className="h-3 w-3" />
      </button>
    </div>
  );
}

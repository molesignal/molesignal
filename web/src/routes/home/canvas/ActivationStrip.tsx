import { ChevronRight, Sparkles } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { ActivationState } from '@/product/activation';

export function ActivationStrip({
  state,
  onOpen,
}: {
  state: ActivationState;
  onOpen: () => void;
}) {
  const { t } = useTranslation('onboarding');
  const remainingCount = state.totalCount - state.completedCount;
  const nextStep = state.steps.find((step) => !step.completed);

  if (remainingCount <= 0) return null;

  return (
    <div
      role="status"
      data-home-surface="activation"
      className="flex min-h-10 flex-wrap items-center gap-x-2.5 gap-y-1 rounded-md bg-[var(--functional-surface)] px-4 py-1 [box-shadow:var(--shadow-functional-surface)]"
    >
      <Sparkles
        aria-hidden="true"
        className="h-3.5 w-3.5 shrink-0 text-indigo-soft"
      />
      <div className="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1">
        <span className="font-sans text-xs font-strong text-tx-1">
          {remainingCount === 1
            ? t('activation.remaining_single')
            : t('activation.remaining_multiple', { count: remainingCount })}
        </span>
        {nextStep && (
          <>
            <span aria-hidden="true" className="text-tx-3">
              ·
            </span>
            <span className="truncate font-sans text-xs text-tx-2">
              {t(`activation.${nextStep.labelKey}`)}
            </span>
          </>
        )}
      </div>
      <button
        type="button"
        onClick={onOpen}
        className="inline-flex h-8 items-center gap-1 rounded px-2 font-sans text-xs font-strong text-blue-soft transition-colors duration-fast hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0 focus-visible:outline-none"
      >
        {t('activation.complete_action')}
        <ChevronRight aria-hidden="true" className="h-3 w-3" />
      </button>
    </div>
  );
}

import {
  CheckCircle2,
  Circle,
  ExternalLink,
  ServerCrash,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { ActivationState } from '@/product/activation';
import { FormDrawer } from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';

export interface QuickStartDrawerProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  state: ActivationState;
  onOpenStep: (to: string) => void;
  onLoadSample: () => void;
  loadingSample: boolean;
}

export function QuickStartDrawer({
  open,
  onOpenChange,
  state,
  onOpenStep,
  onLoadSample,
  loadingSample,
}: QuickStartDrawerProps) {
  const { t } = useTranslation('onboarding');
  const complete = state.completedCount === state.totalCount;
  const percent = Math.round(
    (state.completedCount / Math.max(state.totalCount, 1)) * 100,
  );

  return (
    <FormDrawer
      open={open}
      onOpenChange={onOpenChange}
      title={t('activation.title')}
      subtitle={t('activation.drawer_description')}
      width={480}
      bodyClassName="p-0"
    >
      <div className="border-b border-bd-0 px-6 py-5">
        <div className="flex items-start gap-3">
          <span
            className={cn(
              'mt-0.5 grid h-9 w-9 shrink-0 place-items-center rounded-full',
              complete
                ? 'bg-green-dim text-green-soft'
                : 'bg-indigo-dim text-indigo-soft',
            )}
          >
            {complete ? (
              <CheckCircle2 aria-hidden="true" className="h-5 w-5" />
            ) : (
              <span className="font-sans text-sm font-display-strong tabular-nums">
                {state.completedCount}
              </span>
            )}
          </span>
          <div className="min-w-0 flex-1">
            <div className="flex items-center justify-between gap-3">
              <div className="font-sans text-sm font-strong text-tx-0">
                {complete
                  ? t('activation.complete_title')
                  : t('activation.title')}
              </div>
              <span className="shrink-0 font-sans text-xs font-strong tabular-nums text-tx-2">
                {t('activation.progress_short', {
                  completed: state.completedCount,
                  total: state.totalCount,
                })}
              </span>
            </div>
            <p className="mt-1 font-sans text-xs leading-relaxed text-tx-2">
              {state.ready ? t('activation.ready') : t('activation.empty')}
            </p>
          </div>
        </div>

        <div className="mt-4 h-1.5 overflow-hidden rounded-full bg-bg-3">
          <div
            className="h-full rounded-full bg-indigo transition-[width] duration-normal"
            style={{ width: `${percent}%` }}
          />
        </div>
      </div>

      <ol className="divide-y divide-bd-0">
        {state.steps.map((step, index) => {
          const loadable =
            step.id === 'sample-data' &&
            !step.completed;
          const pending = loadable && loadingSample;

          return (
            <li key={step.id}>
              <button
                type="button"
                disabled={pending}
                onClick={() =>
                  loadable ? onLoadSample() : onOpenStep(step.to)
                }
                className="group flex min-h-[92px] w-full items-start gap-3 px-6 py-4 text-left transition-colors duration-fast enabled:hover:bg-bg-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-indigo disabled:cursor-not-allowed disabled:opacity-60"
              >
                <span className="relative mt-0.5 grid h-7 w-7 shrink-0 place-items-center">
                  {step.backendPending && !step.completed ? (
                    <ServerCrash
                      aria-hidden="true"
                      className="h-5 w-5 text-yellow-soft"
                    />
                  ) : step.completed ? (
                    <CheckCircle2
                      aria-hidden="true"
                      className="h-5 w-5 text-green-soft"
                    />
                  ) : (
                    <>
                      <Circle
                        aria-hidden="true"
                        className="h-5 w-5 text-tx-3"
                      />
                      <span className="type-micro absolute font-sans font-strong text-tx-2">
                        {index + 1}
                      </span>
                    </>
                  )}
                </span>

                <span className="min-w-0 flex-1">
                  <span className="flex items-center justify-between gap-3">
                    <span className="font-sans text-sm font-strong text-tx-0">
                      {t(`activation.${step.labelKey}`)}
                    </span>
                    <span
                      className={cn(
                        'shrink-0 font-sans text-xs font-strong',
                        step.completed
                          ? 'text-green-soft'
                          : 'text-blue-soft',
                      )}
                    >
                      {loadable
                        ? pending
                          ? t('activation.loading')
                          : t('activation.load')
                        : step.completed
                          ? t('activation.done')
                          : t('activation.open')}
                    </span>
                  </span>
                  <span className="mt-1.5 block font-sans text-xs leading-relaxed text-tx-2">
                    {t(`activation.${step.descriptionKey}`)}
                  </span>
                </span>

                {!loadable && (
                  <ExternalLink
                    aria-hidden="true"
                    className="mt-0.5 h-3.5 w-3.5 shrink-0 text-tx-3 transition-colors group-hover:text-tx-1"
                  />
                )}
              </button>
            </li>
          );
        })}
      </ol>
    </FormDrawer>
  );
}

import { ChevronRight, CircleHelp, X } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { TraceRedScope } from '@/api/traceRed';
import { cn } from '@/shell/lib/cn';
import { QuerySplitToggle } from '@/shell/queryTabs';
import { ViewTabs } from '@/shell/tabs/ViewTabs';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/shell/ui/tooltip';

import { describeTraceFilter } from '../inClause';
import type { DrillStep } from './model';

/** How a step of the path reads: the comparison it makes, or the text it looks for. */
function stepLabel(step: DrillStep): string {
  return step.clause ? describeTraceFilter(step.clause) : step.text;
}

/**
 * Where the drill stands: every step that narrowed the query, from all the
 * requests down. Pressing a step goes back up to it; its cross takes that
 * one step away and keeps the rest. Beside it, which Spans count as requests,
 * after the switch that splits the page.
 */
export function RedDrillPath({
  steps,
  scope,
  onScopeChange,
  onKeep,
  onRemove,
}: {
  steps: readonly DrillStep[];
  scope: TraceRedScope;
  onScopeChange: (scope: TraceRedScope) => void;
  /** Go back up to the first `count` steps. */
  onKeep: (count: number) => void;
  onRemove: (index: number) => void;
}) {
  const { t } = useTranslation('traces');

  return (
    <div
      data-red-path
      className="flex min-h-11 flex-wrap items-center justify-between gap-x-4 gap-y-2 rounded-md bg-[var(--functional-surface)] px-4 py-2"
    >
      <nav aria-label={t('explore.red.path.label')} className="min-w-0">
        <ol className="flex min-w-0 flex-wrap items-center gap-1 font-sans text-xs">
          <li>
            <button
              type="button"
              disabled={steps.length === 0}
              aria-current={steps.length === 0 ? 'step' : undefined}
              onClick={() => onKeep(0)}
              className={cn(
                'inline-flex h-7 items-center rounded-md px-2 outline-none transition-colors',
                steps.length === 0
                  ? 'font-strong text-tx-0'
                  : 'font-medium text-indigo-soft hover:bg-bg-2 focus-visible:bg-bg-3',
              )}
            >
              {t('explore.red.path.root')}
            </button>
          </li>
          {steps.map((step, index) => {
            const label = stepLabel(step);
            const last = index === steps.length - 1;

            return (
              <li key={`${index}:${step.text}`} className="flex min-w-0 items-center gap-1">
                <ChevronRight aria-hidden className="h-3.5 w-3.5 shrink-0 text-tx-3" />
                <span
                  data-red-step={index}
                  className="inline-flex h-7 min-w-0 max-w-[360px] items-center rounded-md bg-[var(--control-surface)]"
                >
                  <button
                    type="button"
                    disabled={last}
                    aria-current={last ? 'step' : undefined}
                    title={last ? label : t('explore.red.path.back_to', { step: label })}
                    onClick={() => onKeep(index + 1)}
                    className={cn(
                      'min-w-0 truncate rounded-l-md pl-2 pr-1 font-medium outline-none',
                      last ? 'text-tx-0' : 'text-tx-1 hover:text-indigo-soft focus-visible:text-indigo-soft',
                    )}
                  >
                    {label}
                  </button>
                  <button
                    type="button"
                    aria-label={t('explore.red.path.remove', { step: label })}
                    title={t('explore.red.path.remove', { step: label })}
                    onClick={() => onRemove(index)}
                    className="inline-flex h-7 w-6 shrink-0 items-center justify-center rounded-r-md text-tx-3 outline-none transition-colors hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0"
                  >
                    <X aria-hidden className="h-3 w-3" />
                  </button>
                </span>
              </li>
            );
          })}
        </ol>
      </nav>
      <div className="flex shrink-0 items-center gap-1">
        <QuerySplitToggle size="sm" />
        <ViewTabs<TraceRedScope>
          label={t('explore.red.scope.label')}
          items={[
            { key: 'entry', label: t('explore.red.scope.entry') },
            { key: 'all', label: t('explore.red.scope.all') },
          ]}
          value={scope}
          onValueChange={onScopeChange}
        />
        <Tooltip>
          <TooltipTrigger asChild>
            <button
              type="button"
              aria-label={t('explore.red.scope.hint')}
              className="inline-flex h-7 w-7 items-center justify-center rounded-md text-tx-3 outline-none transition-colors hover:bg-bg-2 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0"
            >
              <CircleHelp aria-hidden className="h-3.5 w-3.5" />
            </button>
          </TooltipTrigger>
          <TooltipContent className="max-w-[320px]">{t('explore.red.scope.hint')}</TooltipContent>
        </Tooltip>
      </div>
    </div>
  );
}

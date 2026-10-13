import { Check, ChevronDown, History } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { formatMicrosActive } from '@/lib/time';
import { cn } from '@/shell/lib/cn';
import { Popover, PopoverContent, PopoverTrigger } from '@/shell/ui/popover';
import { createTimeStore, TimeScopeContext, type TimeWindow } from '@/stores/useTimeStore';
import { formatTimeWindowLabel, TimeRangePanel } from '@/time/TimePicker';

import {
  BASELINE_RELATIONS,
  type CompareBaseline,
  type Period,
  periodWindow,
  relationFits,
} from './periods';

/** What a baseline is called on its control. The times it spans are said beside the control. */
export function baselineLabel(
  baseline: CompareBaseline,
  t: (key: string) => string,
  tCommon: (key: string) => string,
): string {
  if (baseline.kind !== 'window') return t(`compare.baselines.${baseline.kind}`);

  return timeRangeLabel(baseline.window, t, tCommon);
}

/**
 * What a time range is called on a control of this page: a quick range by its
 * name, a start and an end of one's own as that. Their times stand beside the
 * control in the reader's time zone, so the control does not repeat them.
 */
export function timeRangeLabel(
  window: TimeWindow,
  t: (key: string) => string,
  tCommon: (key: string) => string,
): string {
  return window.mode === 'absolute' ? t('compare.baselines.custom') : formatTimeWindowLabel(window, tCommon);
}

/**
 * Which stretch of time a period is compared against.
 *
 * It is set the way the period itself is, with the same quick ranges and the
 * same start and end of one's own, so any two stretches can be set against
 * each other. Over them stand the baselines that follow the period: the
 * stretch before it, and the same stretch a day or a week earlier.
 */
export function BaselinePicker({
  value,
  comparison,
  resolved,
  onChange,
  className,
}: {
  value: CompareBaseline;
  /** The period the baseline is for: it decides which relations say anything. */
  comparison: Period;
  /** The stretch the baseline stands for now: what the start and the end begin from. */
  resolved: Period;
  onChange: (baseline: CompareBaseline) => void;
  className?: string | undefined;
}) {
  const { t } = useTranslation('profiles');
  const { t: tCommon } = useTranslation('common');
  const [open, setOpen] = React.useState(false);

  // The time panel every page has, pointed at the baseline. It shows the baseline as it stands
  // (a relation as the stretch it comes to), and what is chosen in it becomes the baseline.
  const shown: TimeWindow = value.kind === 'window' ? value.window : periodWindow(resolved);
  const store = React.useMemo(
    () => createTimeStore({ window: shown }),
    // eslint-disable-next-line react-hooks/exhaustive-deps -- made once; kept in step below
    [],
  );
  const onChangeRef = React.useRef(onChange);
  onChangeRef.current = onChange;

  React.useLayoutEffect(() => {
    store.setState({
      window: { from: shown.from, to: shown.to, mode: shown.mode },
      setWindow: (window) => onChangeRef.current({ kind: 'window', window }),
    });
  }, [store, shown.from, shown.to, shown.mode]);

  const label = baselineLabel(value, t, tCommon);
  const spans = `${formatMicrosActive(resolved.from, false)} – ${formatMicrosActive(resolved.to, false)}`;

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button
          type="button"
          data-compare-baseline={value.kind}
          // Named with its times for a reader who cannot see them beside it.
          aria-label={t('compare.baseline_aria', { value: `${label}, ${spans}` })}
          // The box the time range of the page is: the two stand one over the other, of one width.
          className={cn(
            'group flex h-8 w-full shrink-0 items-center gap-2 rounded-md border-0 bg-[var(--control-surface)] px-[10px] font-sans text-[13px] font-strong text-tx-1 transition-colors duration-fast hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0 data-[state=open]:bg-bg-3 data-[state=open]:text-tx-0',
            className,
          )}
        >
          <History aria-hidden="true" className="h-3.5 w-3.5 shrink-0 text-tx-3" />
          <span className="min-w-0 flex-1 truncate text-left">{label}</span>
          <ChevronDown
            aria-hidden="true"
            className="h-3.5 w-3.5 shrink-0 text-tx-3 transition-transform duration-fast group-data-[state=open]:rotate-180"
          />
        </button>
      </PopoverTrigger>
      <PopoverContent
        align="start"
        sideOffset={8}
        collisionPadding={12}
        aria-label={t('compare.baseline_title')}
        data-compare-baseline-panel
        data-time-picker-surface="workbench"
        className="w-[600px] max-w-[calc(100vw-24px)] overflow-hidden rounded-md p-0"
      >
        <div className="px-3 pb-2 pt-3 font-sans text-sm font-strong leading-5 text-tx-0">
          {t('compare.baseline_title')}
        </div>

        <section className="px-3 pb-2 pt-1">
          <div className="px-2 pb-2 font-sans text-xs font-strong text-tx-3">
            {t('compare.baseline_relative')}
          </div>
          <div className="grid grid-cols-1 gap-0.5 sm:grid-cols-3">
            {BASELINE_RELATIONS.map((relation) => {
              const selected = value.kind === relation;
              // A baseline inside the period it is compared with says nothing about it.
              const fits = relationFits(relation, comparison);

              return (
                <button
                  key={relation}
                  type="button"
                  data-compare-baseline-option={relation}
                  aria-pressed={selected}
                  disabled={!fits}
                  title={fits ? undefined : t('compare.baseline_overlaps')}
                  onClick={() => {
                    onChange({ kind: relation });
                    setOpen(false);
                  }}
                  className={cn(
                    'flex h-8 items-center gap-2 rounded-sm px-[10px] text-left font-sans text-xs font-body transition-colors duration-fast disabled:cursor-not-allowed disabled:text-tx-3',
                    selected
                      ? 'bg-[var(--floating-item-selected)] font-strong text-indigo-soft'
                      : 'text-tx-1 enabled:hover:bg-[var(--floating-item-hover)] enabled:hover:text-tx-0 enabled:focus-visible:bg-[var(--floating-item-hover)] enabled:focus-visible:text-tx-0',
                  )}
                >
                  <span className="min-w-0 flex-1 truncate">{t(`compare.baselines.${relation}`)}</span>
                  {selected && <Check aria-hidden="true" className="h-3.5 w-3.5 shrink-0" />}
                </button>
              );
            })}
          </div>
        </section>

        {/* The quick ranges and the start and end the period itself is set with. */}
        <TimeScopeContext.Provider value={store}>
          <TimeRangePanel onDone={() => setOpen(false)} />
        </TimeScopeContext.Provider>
      </PopoverContent>
    </Popover>
  );
}

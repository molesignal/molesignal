import { ChevronDown } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { RumField, RumMeasureFn } from '@/api/rum';
import { cn } from '@/shell/lib/cn';
import { Popover, PopoverContent, PopoverTrigger } from '@/shell/ui/popover';

import { CHIP, FieldList } from './FieldPicker';
import { COUNT, MEASURE_FNS, measureFields, type RumMeasureChoice } from './spec';

/**
 * One number asked of the rows as a control: its function, and the field it takes. It opens the
 * functions, and under them the fields that function can take; `count` takes none, so it is
 * chosen at once, and so is another function of the field already taken (a median becomes a 95th
 * percentile). With a `trigger` of its own it is the way to add a measure instead.
 */
export function MeasurePicker({
  measure,
  fields,
  prefix,
  label,
  testId,
  trigger,
  onSelect,
}: {
  measure: RumMeasureChoice | null;
  fields: readonly RumField[];
  prefix?: string;
  label: string;
  testId?: string;
  trigger?: React.ReactNode;
  onSelect: (measure: RumMeasureChoice) => void;
}) {
  const { t } = useTranslation('rum');
  const [open, setOpen] = React.useState(false);
  const [fn, setFn] = React.useState<RumMeasureFn>(measure?.fn ?? 'count');

  // Opened again, it starts from what is chosen.
  React.useEffect(() => {
    if (open) setFn(measure?.fn ?? 'count');
  }, [measure?.fn, open]);

  const commit = (next: RumMeasureChoice) => {
    onSelect(next);
    setOpen(false);
  };

  const pick = (candidate: RumMeasureFn) => {
    if (candidate === 'count') return commit(COUNT);

    const taken = measure?.field;

    if (taken && measureFields(candidate, fields).some((field) => field.name === taken)) {
      return commit({ fn: candidate, field: taken });
    }

    setFn(candidate);
  };

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        {trigger ?? (
          <button type="button" aria-label={label} data-rum-viz-control={testId} className={CHIP}>
            {prefix ? <span className="shrink-0 text-tx-3">{prefix}</span> : null}
            <span className="shrink-0 font-semibold text-indigo-soft">{measure?.fn}</span>
            {measure?.field ? <span className="min-w-0 max-w-[260px] truncate font-mono">{measure.field}</span> : null}
            <ChevronDown aria-hidden="true" className="h-[14px] w-[14px] shrink-0 opacity-50" />
          </button>
        )}
      </PopoverTrigger>
      <PopoverContent align="start" className="w-auto p-0">
        <div role="group" aria-label={t('explorer.viz.function')} className="flex w-[300px] flex-wrap gap-1 p-2">
          {MEASURE_FNS.map((candidate) => (
            <button
              key={candidate}
              type="button"
              aria-pressed={candidate === fn}
              onClick={() => pick(candidate)}
              className={cn(
                'h-7 rounded px-2 font-mono text-xs transition-colors',
                candidate === fn
                  ? 'bg-indigo-dim font-semibold text-indigo-soft'
                  : 'bg-[var(--control-surface)] text-tx-2 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0',
              )}
            >
              {candidate}
            </button>
          ))}
        </div>
        {fn !== 'count' && (
          <FieldList
            fields={measureFields(fn, fields)}
            selected={measure?.fn === fn ? (measure.field ?? null) : null}
            onSelect={(name) => commit({ fn, field: name })}
          />
        )}
      </PopoverContent>
    </Popover>
  );
}

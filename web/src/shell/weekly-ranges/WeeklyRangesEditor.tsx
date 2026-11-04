import { useTranslation } from 'react-i18next';

import { FieldArray, FormField, FormInput } from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';
import type { WeeklyRange } from '@/types/alerting';

import {
  WEEKDAYS,
  newRange,
  normalizeClock,
  rangeProblem,
  weekdayLabel,
} from './model';

/** At most this many ranges, as the server takes them. */
const MAX_RANGES = 16;

/**
 * Weekly time ranges in local time, each with its days and a start and end to the minute.
 * An end not after the start runs past midnight; `24:00` closes the day.
 */
export function WeeklyRangesEditor({
  ranges,
  onChange,
  minRanges = 1,
}: {
  ranges: WeeklyRange[];
  onChange: (ranges: WeeklyRange[]) => void;
  minRanges?: number;
}) {
  const { t } = useTranslation('common');

  return (
    <div className="flex flex-col gap-2" data-weekly-ranges>
      <FieldArray<WeeklyRange>
        items={ranges}
        onChange={onChange}
        minItems={minRanges}
        maxItems={MAX_RANGES}
        addLabel={t('weekly_ranges.add')}
        removeLabel={t('weekly_ranges.remove')}
        newItem={newRange}
        renderItem={(range, index, setRange) => (
          <RangeRow range={range} index={index} onChange={setRange} />
        )}
      />
      <p className="font-sans text-xs text-tx-3">{t('weekly_ranges.hint')}</p>
    </div>
  );
}

function RangeRow({
  range,
  index,
  onChange,
}: {
  range: WeeklyRange;
  index: number;
  onChange: (range: WeeklyRange) => void;
}) {
  const { t, i18n } = useTranslation('common');
  const problem = rangeProblem(range);

  return (
    <div data-weekly-range={index} className="flex flex-col gap-2 rounded-md bg-bg-2 p-2.5">
      <div role="group" aria-label={t('weekly_ranges.weekdays')} className="flex flex-wrap gap-1">
        {WEEKDAYS.map((day) => {
          const on = (range.weekdayMask & (1 << day)) !== 0;

          return (
            <button
              key={day}
              type="button"
              aria-pressed={on}
              onClick={() => onChange({ ...range, weekdayMask: range.weekdayMask ^ (1 << day) })}
              className={cn(
                'min-w-[44px] rounded-md px-2.5 py-1.5 font-sans text-xs outline-none transition-colors',
                on
                  ? 'bg-indigo-dim font-strong text-indigo-soft'
                  : 'bg-[var(--control-surface)] text-tx-2 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3',
              )}
            >
              {weekdayLabel(day, i18n.language)}
            </button>
          );
        })}
      </div>
      <div className="grid grid-cols-2 gap-2">
        <FormField label={t('weekly_ranges.start')}>
          <FormInput
            value={range.start}
            inputMode="numeric"
            maxLength={5}
            placeholder="09:00"
            data-range-start
            onChange={(event) => onChange({ ...range, start: event.target.value })}
            onBlur={(event) => onChange({ ...range, start: normalizeClock(event.target.value) })}
          />
        </FormField>
        <FormField label={t('weekly_ranges.end')}>
          <FormInput
            value={range.end}
            inputMode="numeric"
            maxLength={5}
            placeholder="18:00"
            data-range-end
            onChange={(event) => onChange({ ...range, end: event.target.value })}
            onBlur={(event) => onChange({ ...range, end: normalizeClock(event.target.value) })}
          />
        </FormField>
      </div>
      {problem && (
        <p role="alert" className="font-sans text-xs text-[var(--red-ink)]">
          {t(`weekly_ranges.problems.${problem}`)}
        </p>
      )}
    </div>
  );
}

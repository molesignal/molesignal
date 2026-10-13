import { useTranslation } from 'react-i18next';

import { FormInput, FormSelect } from '@/shell/FormDrawer';

import { INTERVAL_UNITS, type Interval, type IntervalUnit } from './schedule';

/** A span of time as people say it: a count and a unit, side by side. */
export function IntervalField({
  label,
  value,
  onChange,
  disabled,
  disabledReason,
}: {
  /** Names both controls for assistive technology; the caption above them is the caller's. */
  label: string;
  value: Interval;
  onChange: (next: Interval) => void;
  disabled?: boolean;
  disabledReason?: string | undefined;
}) {
  const { t } = useTranslation('pipelines');

  return (
    <div className="flex min-w-0 gap-[6px]">
      <FormInput
        type="number"
        min={1}
        step={1}
        required
        inputMode="numeric"
        aria-label={t('settings.interval_count', { label })}
        value={Number.isFinite(value.count) ? value.count : ''}
        onChange={(event) =>
          onChange({ ...value, count: event.target.value === '' ? Number.NaN : Number(event.target.value) })
        }
        className="w-[76px] shrink-0 tabular-nums"
        disabled={disabled}
        disabledReason={disabledReason}
      />
      <FormSelect
        ariaLabel={t('settings.interval_unit', { label })}
        value={value.unit}
        onChange={(unit) => onChange({ ...value, unit: unit as IntervalUnit })}
        options={INTERVAL_UNITS.map((unit) => ({
          value: unit,
          label: t(`settings.units.${unit}`),
        }))}
        className="min-w-[104px]"
        {...(disabled !== undefined ? { disabled } : {})}
        disabledReason={disabledReason}
      />
    </div>
  );
}

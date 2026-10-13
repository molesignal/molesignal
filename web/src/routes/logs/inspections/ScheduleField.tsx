import { useTranslation } from 'react-i18next';

import type { InspectionLimits } from '@/api/logInspections';
import { uiLabelClass } from '@/shell/chrome';
import { FormField, FormSelect } from '@/shell/FormDrawer';

import {
  HOURS_OF_DAY,
  SCHEDULE_KINDS,
  WEEKDAYS,
  durationLabel,
  intervalOptions,
  minuteChoices,
  timeOfDay,
  twoDigits,
  type InspectionDraft,
  type ScheduleKind,
} from './model';

interface ScheduleFieldProps {
  draft: InspectionDraft;
  limits: InspectionLimits;
  /** The time zone a time of day is read in. */
  timezone: string;
  onChange: (change: Partial<InspectionDraft>) => void;
}

/**
 * How often the inspection runs: every so many hours, or at a time of day,
 * daily or weekly. An instance may forbid the shorter intervals; those are not
 * offered.
 */
export function ScheduleField({ draft, limits, timezone, onChange }: ScheduleFieldProps) {
  const { t } = useTranslation('logs');
  const intervals = intervalOptions(limits);
  // A kind the instance cannot run at all (every interval too short) is left out.
  const kinds = SCHEDULE_KINDS.filter((kind) => {
    if (kind === 'interval') return intervals.length > 0;
    return (kind === 'daily' ? 86_400 : 7 * 86_400) >= limits.min_interval_secs;
  });
  const timed = draft.scheduleKind !== 'interval';

  return (
    <div data-inspection-schedule={draft.scheduleKind} className="grid grid-cols-3 gap-3">
      <FormField label={t('inspections.editor.schedule')}>
        <FormSelect
          ariaLabel={t('inspections.editor.schedule')}
          value={draft.scheduleKind}
          onChange={(value) => onChange({ scheduleKind: value as ScheduleKind })}
          options={kinds.map((kind) => ({
            value: kind,
            label: t(`inspections.schedule_kinds.${kind}`),
          }))}
        />
      </FormField>
      {draft.scheduleKind === 'interval' && (
        <FormField label={t('inspections.editor.interval')}>
          <FormSelect
            ariaLabel={t('inspections.editor.interval')}
            value={String(draft.everyMinutes)}
            onChange={(value) => onChange({ everyMinutes: Number(value) })}
            options={intervals.map((minutes) => ({
              value: String(minutes),
              label: durationLabel(t, minutes * 60),
            }))}
          />
        </FormField>
      )}
      {draft.scheduleKind === 'weekly' && (
        <FormField label={t('inspections.editor.weekday')}>
          <FormSelect
            ariaLabel={t('inspections.editor.weekday')}
            value={String(draft.weekday)}
            onChange={(value) => onChange({ weekday: Number(value) })}
            options={WEEKDAYS.map((weekday) => ({
              value: String(weekday),
              label: t(`inspections.weekdays.${weekday}`),
            }))}
          />
        </FormField>
      )}
      {timed && (
        // An hour and a minute of the product's own: a time field of the browser follows the
        // browser's clock and type, not the product's. Two controls, so a group, not a label.
        <div
          role="group"
          aria-label={t('inspections.editor.time')}
          data-inspection-time={timeOfDay(draft.hour, draft.minute)}
          className="flex flex-col gap-1.5"
        >
          <span className={uiLabelClass}>{t('inspections.editor.time')}</span>
          <div className="flex items-center gap-1.5">
            <FormSelect
              ariaLabel={t('inspections.editor.hour')}
              value={String(draft.hour)}
              onChange={(value) => onChange({ hour: Number(value) })}
              options={HOURS_OF_DAY.map((hour) => ({ value: String(hour), label: twoDigits(hour) }))}
            />
            <span aria-hidden="true" className="font-sans text-sm text-tx-2">
              :
            </span>
            <FormSelect
              ariaLabel={t('inspections.editor.minute')}
              value={String(draft.minute)}
              onChange={(value) => onChange({ minute: Number(value) })}
              options={minuteChoices(draft.minute).map((minute) => ({
                value: String(minute),
                label: twoDigits(minute),
              }))}
            />
          </div>
          <span className="text-xs leading-relaxed text-tx-3">
            {t('inspections.editor.time_hint', { timezone })}
          </span>
        </div>
      )}
    </div>
  );
}

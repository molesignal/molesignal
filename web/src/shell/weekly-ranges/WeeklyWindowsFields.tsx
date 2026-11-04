import { useTranslation } from 'react-i18next';

import { FormField, FormSelect } from '@/shell/FormDrawer';
import { TimezoneField } from '@/shell/TimezoneField';
import type { TimeCondition, WeeklyWindows } from '@/types/alerting';

import { HolidayField } from './HolidayField';
import { WORKDAYS_MASK, rangeProblem } from './model';
import { WeeklyRangesEditor } from './WeeklyRangesEditor';

/** Weekday office hours in the reader's timezone: where support hours and time conditions start. */
export function officeHours(): WeeklyWindows {
  return {
    timezone: Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC',
    ranges: [{ weekdayMask: WORKDAYS_MASK, start: '09:00', end: '18:00' }],
  };
}

/** The server takes weekly windows with at least one range, every range sound, and a calendar when holidays count. */
export function windowsInvalid(windows: WeeklyWindows): boolean {
  return (
    windows.ranges.length === 0 ||
    windows.ranges.some((range) => rangeProblem(range) !== null) ||
    (windows.calendar != null && !windows.calendar.calendarId)
  );
}

/** Weekly ranges and the timezone they are read in. */
export function WeeklyWindowsFields({
  windows,
  onChange,
}: {
  windows: WeeklyWindows;
  onChange: (windows: WeeklyWindows) => void;
}) {
  const { t } = useTranslation('common');

  return (
    <div className="flex flex-col gap-2" data-weekly-windows>
      <FormField label={t('weekly_ranges.timezone')}>
        <TimezoneField
          label={t('weekly_ranges.timezone')}
          value={windows.timezone}
          onChange={(timezone) => onChange({ ...windows, timezone })}
        />
      </FormField>
      <WeeklyRangesEditor ranges={windows.ranges} onChange={(ranges) => onChange({ ...windows, ranges })} />
      <HolidayField value={windows.calendar} onChange={(calendar) => onChange({ ...windows, calendar })} />
    </div>
  );
}

type TimeMode = 'any' | TimeCondition['mode'];

/** When something applies: any time, or only inside or outside some weekly hours. */
export function TimeConditionField({
  value,
  onChange,
  hint,
}: {
  value: TimeCondition | null;
  onChange: (value: TimeCondition | null) => void;
  hint?: string;
}) {
  const { t } = useTranslation('common');

  return (
    <div className="flex flex-col gap-2" data-time-condition={value?.mode ?? 'any'}>
      <FormField label={t('time_condition.label')} {...(hint ? { hint } : {})}>
        <FormSelect
          ariaLabel={t('time_condition.label')}
          value={value?.mode ?? 'any'}
          onChange={(next) => {
            const mode = next as TimeMode;
            onChange(mode === 'any' ? null : { ...(value ?? officeHours()), mode });
          }}
          options={[
            { value: 'any', label: t('time_condition.any') },
            { value: 'inside', label: t('time_condition.inside') },
            { value: 'outside', label: t('time_condition.outside') },
          ]}
        />
      </FormField>
      {value && (
        <WeeklyWindowsFields windows={value} onChange={(windows) => onChange({ ...windows, mode: value.mode })} />
      )}
    </div>
  );
}

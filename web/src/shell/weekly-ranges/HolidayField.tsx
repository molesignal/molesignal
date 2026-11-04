import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';

import * as holidaysApi from '@/api/holidays';
import { FormField, FormSelect } from '@/shell/FormDrawer';
import type { CalendarUse } from '@/types/alerting';

type HolidayMode = 'none' | CalendarUse['holidays'];

/** The holiday calendars of the organization, shared by every form that picks one. */
export function useHolidayCalendars(enabled = true) {
  return useQuery({ queryKey: ['holiday-calendars'], queryFn: holidaysApi.list, enabled });
}

/**
 * Whether some weekly hours count by a holiday calendar: not at all, with holidays left out (a
 * make-up workday counts as a Monday), or on holidays only.
 */
export function HolidayField({
  value,
  onChange,
}: {
  value: CalendarUse | null | undefined;
  onChange: (value: CalendarUse | null) => void;
}) {
  const { t } = useTranslation('common');
  const calendars = useHolidayCalendars();
  const choices = calendars.data ?? [];
  const mode: HolidayMode = value?.holidays ?? 'none';
  const none = choices.length === 0;

  return (
    <div className="grid gap-3 sm:grid-cols-2" data-holidays={mode}>
      <FormField label={t('holidays.label')} hint={t(none ? 'holidays.no_calendars' : `holidays.hints.${mode}`)}>
        <FormSelect
          ariaLabel={t('holidays.label')}
          value={mode}
          onChange={(next) => {
            // Radix reports '' when a value and its option arrive in one render; nobody picks nothing.
            if (!next) return;
            const holidays = next as HolidayMode;
            if (holidays === 'none') onChange(null);
            else onChange({ calendarId: value?.calendarId ?? choices[0]?.id ?? '', holidays });
          }}
          options={(['none', 'exclude', 'only'] as const).map((option) => ({
            value: option,
            label: t(`holidays.modes.${option}`),
            ...(option !== 'none' && none ? { disabled: true, disabledReason: t('holidays.no_calendars') } : {}),
          }))}
        />
      </FormField>
      {value && (
        <FormField label={t('holidays.calendar')}>
          <FormSelect
            ariaLabel={t('holidays.calendar')}
            value={value.calendarId}
            onChange={(calendar_id) => {
              if (calendar_id) onChange({ ...value, calendarId: calendar_id });
            }}
            options={choices.map((calendar) => ({ value: calendar.id, label: calendar.name }))}
          />
        </FormField>
      )}
    </div>
  );
}

import { useTranslation } from 'react-i18next';

import { DateTimePicker } from '@/shell/DateTimePicker';
import { FormField, FormInput, FormRow, FormSelect } from '@/shell/FormDrawer';
import { TimezoneField } from '@/shell/TimezoneField';
import { HolidayField, useHolidayCalendars } from '@/shell/weekly-ranges/HolidayField';
import { WEEKDAYS, normalizeClock, rangeProblem, weekdayLabel } from '@/shell/weekly-ranges/model';
import { WeeklyRangesEditor } from '@/shell/weekly-ranges/WeeklyRangesEditor';
import type { MuteWindow } from '@/types/alerting';

import { localInputToMicros, microsToLocalInput } from '../schedule';
import { MAX_MONTHLY_MINUTES, MONTH_ORDINALS, type SilenceKind, kindOfWindow, windowOfKind } from './window';

type MonthlyWindow = Extract<MuteWindow, { type: 'monthly' }>;

const CLOCK = /^([01]\d|2[0-3]):[0-5]\d$/;

/** What keeps the server from taking a window, as a key under `silences.errors`; null when it is fine. */
export function windowProblem(window: MuteWindow): string | null {
  if (window.type === 'fixed') return window.end <= window.start ? 'window_range' : null;
  if (window.type === 'weekly') {
    if (window.calendar && !window.calendar.calendarId) return 'calendar';
    return window.ranges.length === 0 || window.ranges.some((range) => rangeProblem(range))
      ? 'window_ranges'
      : null;
  }
  const durationOk =
    Number.isInteger(window.durationMinutes)
    && window.durationMinutes >= 1
    && window.durationMinutes <= MAX_MONTHLY_MINUTES;
  return CLOCK.test(window.start) && durationOk ? null : 'monthly';
}

/** When a silence applies: a fixed stretch, every week at some times, or once a month. */
export function SilenceWindowFields({
  window,
  onChange,
}: {
  window: MuteWindow;
  onChange: (window: MuteWindow) => void;
}) {
  const { t } = useTranslation('alerts');
  const calendars = useHolidayCalendars();
  const kind = kindOfWindow(window);

  return (
    <>
      <FormField label={t('silences.fields.window_type')}>
        <FormSelect
          value={kind}
          onChange={(value) =>
            onChange(
              windowOfKind(
                value as SilenceKind,
                window.type === 'fixed' ? undefined : window.timezone,
                calendars.data?.[0]?.id ?? '',
              ),
            )
          }
          options={[
            { value: 'fixed', label: t('silences.window_kinds.fixed') },
            { value: 'weekly', label: t('silences.window_kinds.weekly') },
            { value: 'monthly', label: t('silences.window_kinds.monthly') },
            {
              value: 'holidays',
              label: t('silences.window_kinds.holidays'),
              ...((calendars.data?.length ?? 0) === 0
                ? { disabled: true, disabledReason: t('silences.no_calendars') }
                : {}),
            },
          ]}
        />
      </FormField>
      {window.type === 'fixed' && (
        <div className="grid grid-cols-2 gap-2">
          <FormField label={t('silences.fields.start')} required>
            <DateTimePicker
              value={microsToLocalInput(window.start)}
              onChange={(value) => onChange({ ...window, start: localInputToMicros(value) })}
              required
            />
          </FormField>
          <FormField label={t('silences.fields.end')} required>
            <DateTimePicker
              value={microsToLocalInput(window.end)}
              onChange={(value) => onChange({ ...window, end: localInputToMicros(value) })}
              required
            />
          </FormField>
        </div>
      )}
      {window.type === 'weekly' && (
        <div className="flex flex-col gap-2">
          <FormField label={t('silences.fields.timezone')}>
            <TimezoneField
              label={t('silences.fields.timezone')}
              value={window.timezone}
              onChange={(timezone) => onChange({ ...window, timezone })}
            />
          </FormField>
          {kind === 'holidays' && window.calendar ? (
            <FormField label={t('silences.fields.calendar')}>
              <FormSelect
                ariaLabel={t('silences.fields.calendar')}
                value={window.calendar.calendarId}
                onChange={(calendar_id) => {
                  // Radix reports '' when a value and its option arrive in one render.
                  if (calendar_id) onChange({ ...window, calendar: { calendarId: calendar_id, holidays: 'only' } });
                }}
                options={(calendars.data ?? []).map((calendar) => ({ value: calendar.id, label: calendar.name }))}
              />
            </FormField>
          ) : (
            <>
              <WeeklyRangesEditor
                ranges={window.ranges}
                onChange={(ranges) => onChange({ ...window, ranges })}
              />
              <HolidayField value={window.calendar} onChange={(calendar) => onChange({ ...window, calendar })} />
            </>
          )}
        </div>
      )}
      {window.type === 'monthly' && <MonthlyFields window={window} onChange={onChange} />}
    </>
  );
}

function MonthlyFields({
  window,
  onChange,
}: {
  window: MonthlyWindow;
  onChange: (window: MuteWindow) => void;
}) {
  const { t, i18n } = useTranslation('alerts');
  const { day } = window;

  return (
    <div className="flex flex-col gap-2" data-monthly-window>
      <FormField label={t('silences.fields.timezone')}>
        <TimezoneField
          label={t('silences.fields.timezone')}
          value={window.timezone}
          onChange={(timezone) => onChange({ ...window, timezone })}
        />
      </FormField>
      <FormRow>
        <FormField label={t('silences.fields.month_day_kind')}>
          <FormSelect
            value={day.kind}
            onChange={(kind) =>
              onChange({
                ...window,
                day: kind === 'date' ? { kind: 'date', day: 1 } : { kind: 'weekday', ordinal: 1, weekday: 0 },
              })
            }
            options={[
              { value: 'date', label: t('silences.month_day_kinds.date') },
              { value: 'weekday', label: t('silences.month_day_kinds.weekday') },
            ]}
          />
        </FormField>
        {day.kind === 'date' ? (
          <FormField label={t('silences.fields.month_date')} hint={t('silences.hints.month_date')}>
            <FormInput
              type="number"
              min={1}
              max={31}
              value={String(day.day)}
              onChange={(event) =>
                onChange({
                  ...window,
                  day: { kind: 'date', day: Math.min(31, Math.max(1, Math.floor(Number(event.target.value)) || 1)) },
                })
              }
            />
          </FormField>
        ) : (
          <div className="grid grid-cols-2 gap-2">
            <FormField label={t('silences.fields.ordinal')}>
              <FormSelect
                value={String(day.ordinal)}
                onChange={(ordinal) => onChange({ ...window, day: { ...day, ordinal: Number(ordinal) } })}
                options={MONTH_ORDINALS.map((ordinal) => ({
                  value: String(ordinal),
                  label: t(`silences.ordinals.${ordinal === -1 ? 'last' : String(ordinal)}`),
                }))}
              />
            </FormField>
            <FormField label={t('silences.fields.weekday')}>
              <FormSelect
                value={String(day.weekday)}
                onChange={(weekday) => onChange({ ...window, day: { ...day, weekday: Number(weekday) } })}
                options={WEEKDAYS.map((weekday) => ({
                  value: String(weekday),
                  label: weekdayLabel(weekday, i18n.language),
                }))}
              />
            </FormField>
          </div>
        )}
      </FormRow>
      <FormRow>
        <FormField label={t('silences.fields.start_time')}>
          <FormInput
            value={window.start}
            inputMode="numeric"
            maxLength={5}
            placeholder="02:00"
            onChange={(event) => onChange({ ...window, start: event.target.value })}
            onBlur={(event) => onChange({ ...window, start: normalizeClock(event.target.value) })}
          />
        </FormField>
        <FormField label={t('silences.fields.duration_minutes')} hint={t('silences.hints.duration_minutes')}>
          <FormInput
            type="number"
            min={1}
            max={MAX_MONTHLY_MINUTES}
            value={String(window.durationMinutes)}
            onChange={(event) =>
              onChange({ ...window, durationMinutes: Math.floor(Number(event.target.value)) || 0 })
            }
          />
        </FormField>
      </FormRow>
    </div>
  );
}

import { CalendarDays, Clock3 } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ChromeButton } from '@/shell/chrome';
import { DisabledControl } from '@/shell/DisabledControl';
import { FormSelect } from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';
import { Calendar } from '@/shell/ui/calendar';
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from '@/shell/ui/popover';

const HOURS = Array.from({ length: 24 }, (_, value) => {
  const label = String(value).padStart(2, '0');
  return { value: label, label };
});
const MINUTES = Array.from({ length: 60 }, (_, value) => {
  const label = String(value).padStart(2, '0');
  return { value: label, label };
});

export interface DateTimePickerProps {
  value: string;
  onChange: (value: string) => void;
  id?: string;
  className?: string;
  placeholder?: string;
  includeSeconds?: boolean;
  disabled?: boolean;
  readOnly?: boolean;
  disabledReason?: React.ReactNode;
  required?: boolean;
  'aria-label'?: string;
  'aria-invalid'?: boolean;
}

/**
 * App-localized date and time selection.
 *
 * Values deliberately use the same local wall-clock representation as
 * `datetime-local` (`YYYY-MM-DDTHH:mm[:ss]`) so callers can migrate without
 * changing their API payloads. Unlike the browser control, all visible and
 * accessible copy follows the application's selected language.
 */
export function DateTimePicker({
  value,
  onChange,
  id,
  className,
  placeholder,
  includeSeconds = false,
  disabled = false,
  readOnly = false,
  disabledReason,
  required,
  'aria-label': ariaLabel,
  'aria-invalid': ariaInvalid,
}: DateTimePickerProps) {
  const { t, i18n } = useTranslation('common');
  const [open, setOpen] = React.useState(false);
  const [draft, setDraft] = React.useState(() =>
    initialDraft(value, includeSeconds),
  );
  const parsedValue = parseLocalDateTime(value);
  const interactionDisabled = disabled || readOnly;
  const language = i18n.resolvedLanguage ?? i18n.language;
  const displayValue = parsedValue
    ? formatDateTimeDisplay(parsedValue, language, includeSeconds)
    : value;

  const changeOpen = (next: boolean) => {
    if (next && interactionDisabled) return;
    if (next) setDraft(initialDraft(value, includeSeconds));
    setOpen(next);
  };

  const setDatePart = (date: Date | undefined) => {
    if (!date) return;
    setDraft(
      new Date(
        date.getFullYear(),
        date.getMonth(),
        date.getDate(),
        draft.getHours(),
        draft.getMinutes(),
        includeSeconds ? draft.getSeconds() : 0,
      ),
    );
  };

  const setTimePart = (
    part: 'hours' | 'minutes' | 'seconds',
    nextValue: string,
  ) => {
    const next = new Date(draft);
    const numericValue = Number(nextValue);
    if (part === 'hours') next.setHours(numericValue);
    if (part === 'minutes') next.setMinutes(numericValue);
    if (part === 'seconds') next.setSeconds(numericValue);
    next.setMilliseconds(0);
    setDraft(next);
  };

  const trigger = (
    <button
      id={id}
      type="button"
      disabled={interactionDisabled}
      aria-disabled={interactionDisabled || undefined}
      aria-readonly={readOnly || undefined}
      aria-required={required || undefined}
      aria-invalid={ariaInvalid || undefined}
      aria-label={
        ariaLabel ??
        t('date_time_picker.open', {
          value: displayValue || placeholder || t('date_time_picker.placeholder'),
        })
      }
      className={cn(
        'flex h-9 w-full min-w-0 items-center gap-2 rounded-md border border-bd-1 bg-bg-2 px-3 text-left font-sans text-sm text-tx-0 transition-colors',
        'enabled:hover:border-bd-2 enabled:hover:bg-bg-3',
        'disabled:pointer-events-none disabled:cursor-not-allowed disabled:border-bd-0 disabled:bg-bg-3 disabled:text-tx-3 disabled:opacity-100',
        ariaInvalid && 'border-red',
        className,
      )}
    >
      <CalendarDays className="h-4 w-4 shrink-0 text-tx-3" />
      <span
        className={cn(
          'min-w-0 flex-1 truncate',
          !displayValue && 'text-tx-3',
        )}
      >
        {displayValue || placeholder || t('date_time_picker.placeholder')}
      </span>
    </button>
  );

  return (
    <Popover open={open} onOpenChange={changeOpen}>
      <DisabledControl
        disabled={interactionDisabled}
        reason={disabledReason}
        className="w-full"
      >
        <PopoverTrigger asChild>{trigger}</PopoverTrigger>
      </DisabledControl>
      <PopoverContent
        align="start"
        collisionPadding={12}
        data-slot="date-time-picker-content"
        className="w-[var(--radix-popover-trigger-width)] min-w-0 max-w-[calc(100vw-24px)] [container-type:inline-size] border-bd-1 bg-bg-1 p-0 text-tx-0 shadow-popup"
      >
        <Calendar
          mode="single"
          className="w-full"
          style={
            {
              '--cell-size': 'clamp(1.5rem, 12cqi, 2rem)',
            } as React.CSSProperties
          }
          classNames={{
            root: 'w-full',
            months: 'w-full',
            month: 'w-full',
            month_grid: 'w-full table-fixed',
            day: 'h-[--cell-size] w-auto aspect-auto',
            day_button: 'mx-auto size-[--cell-size] min-w-0',
          }}
          selected={draft}
          month={draft}
          onMonthChange={(month) =>
            setDraft(
              new Date(
                month.getFullYear(),
                month.getMonth(),
                Math.min(draft.getDate(), daysInMonth(month)),
                draft.getHours(),
                draft.getMinutes(),
                includeSeconds ? draft.getSeconds() : 0,
              ),
            )
          }
          onSelect={setDatePart}
        />

        <div className="border-t border-bd-0 px-3 py-3">
          <div className="mb-2 flex items-center gap-2 font-sans text-xs font-strong text-tx-2">
            <Clock3 className="h-3.5 w-3.5 text-tx-3" />
            {t('date_time_picker.time')}
          </div>
          <div
            className={cn(
              'grid items-end gap-2',
              includeSeconds ? 'grid-cols-3' : 'grid-cols-2',
            )}
          >
            <TimePartSelect
              label={t('date_time_picker.hour')}
              value={String(draft.getHours()).padStart(2, '0')}
              options={HOURS}
              onChange={(next) => setTimePart('hours', next)}
            />
            <TimePartSelect
              label={t('date_time_picker.minute')}
              value={String(draft.getMinutes()).padStart(2, '0')}
              options={MINUTES}
              onChange={(next) => setTimePart('minutes', next)}
            />
            {includeSeconds && (
              <TimePartSelect
                label={t('date_time_picker.second')}
                value={String(draft.getSeconds()).padStart(2, '0')}
                options={MINUTES}
                onChange={(next) => setTimePart('seconds', next)}
              />
            )}
          </div>
        </div>

        <div className="flex flex-wrap items-center justify-between gap-2 border-t border-bd-0 bg-bg-2 px-3 py-2.5">
          <ChromeButton
            size="sm"
            variant="ghost"
            onClick={() => {
              onChange('');
              setOpen(false);
            }}
          >
            {t('date_time_picker.clear')}
          </ChromeButton>
          <div className="flex items-center gap-2">
            <ChromeButton
              size="sm"
              variant="default"
              onClick={() => setOpen(false)}
            >
              {t('actions.cancel')}
            </ChromeButton>
            <ChromeButton
              size="sm"
              variant="primary"
              onClick={() => {
                onChange(formatLocalDateTime(draft, includeSeconds));
                setOpen(false);
              }}
            >
              {t('date_time_picker.apply')}
            </ChromeButton>
          </div>
        </div>
      </PopoverContent>
    </Popover>
  );
}

function TimePartSelect({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: string;
  options: Array<{ value: string; label: string }>;
  onChange: (value: string) => void;
}) {
  return (
    <div className="min-w-0">
      <div className="mb-1 font-sans text-xs text-tx-3">{label}</div>
      <FormSelect
        value={value}
        onChange={onChange}
        options={options}
        ariaLabel={label}
        className="h-8 min-w-0 font-mono text-xs"
      />
    </div>
  );
}

function initialDraft(value: string, includeSeconds: boolean): Date {
  const parsed = parseLocalDateTime(value);
  if (parsed) return parsed;
  const now = new Date();
  now.setMilliseconds(0);
  if (!includeSeconds) now.setSeconds(0);
  return now;
}

function daysInMonth(date: Date): number {
  return new Date(date.getFullYear(), date.getMonth() + 1, 0).getDate();
}

export function parseLocalDateTime(value: string): Date | null {
  const match =
    /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})(?::(\d{2}))?$/.exec(
      value,
    );
  if (!match) return null;

  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  const hours = Number(match[4]);
  const minutes = Number(match[5]);
  const seconds = Number(match[6] ?? 0);
  const date = new Date(0);
  date.setHours(0, 0, 0, 0);
  date.setFullYear(year, month - 1, 1);
  date.setDate(day);
  date.setHours(hours, minutes, seconds, 0);

  if (
    date.getFullYear() !== year ||
    date.getMonth() !== month - 1 ||
    date.getDate() !== day ||
    date.getHours() !== hours ||
    date.getMinutes() !== minutes ||
    date.getSeconds() !== seconds
  ) {
    return null;
  }
  return date;
}

export function formatLocalDateTime(
  date: Date,
  includeSeconds = false,
): string {
  const pad = (part: number) => String(part).padStart(2, '0');
  const datePart = [
    String(date.getFullYear()).padStart(4, '0'),
    pad(date.getMonth() + 1),
    pad(date.getDate()),
  ].join('-');
  const timePart = [pad(date.getHours()), pad(date.getMinutes())];
  if (includeSeconds) timePart.push(pad(date.getSeconds()));
  return `${datePart}T${timePart.join(':')}`;
}

export function formatDateTimeDisplay(
  date: Date,
  language: string | undefined,
  includeSeconds = false,
): string {
  return new Intl.DateTimeFormat(toIntlLocale(language), {
    dateStyle: 'medium',
    timeStyle: includeSeconds ? 'medium' : 'short',
  }).format(date);
}

export function toIntlLocale(language: string | undefined): 'en-US' | 'zh-CN' {
  return language?.toLowerCase().startsWith('zh') ? 'zh-CN' : 'en-US';
}

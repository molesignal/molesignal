import { Check, ChevronsUpDown } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type * as meApi from '@/api/me';
import { formatMicros, resolveTimezone } from '@/lib/time';
import { DisabledControl } from '@/shell/DisabledControl';
import { FormSelect } from '@/shell/FormDrawer';
import { cn } from '@/shell/lib/cn';
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from '@/shell/ui/command';
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from '@/shell/ui/popover';

const DATE_SAMPLES: Record<meApi.PreferenceDateFormat, string> = {
  yyyy_mm_dd_dash: '2026-07-27',
  yyyy_mm_dd_slash: '2026/07/27',
  dd_mm_yyyy_slash: '27/07/2026',
  mm_dd_yyyy_slash: '07/27/2026',
};

export interface PreferenceOption<T extends string> {
  value: T;
  label: string;
  disabled?: boolean;
  disabledReason?: string | undefined;
}

export function DateTimeFormatPopover({
  value,
  onChange,
  disabled = false,
  disabledReason,
}: {
  value: meApi.UserPreferences;
  onChange: (patch: Partial<meApi.UserPreferences>) => void;
  disabled?: boolean;
  disabledReason?: string | undefined;
}) {
  const { t } = useTranslation('settings-admin');
  const [open, setOpen] = React.useState(false);
  const timeLabel =
    value.time_format === 'local_12h'
      ? t('preferences.values.time_12h')
      : t('preferences.values.time_24h');
  const preview = formatMicros(
    Date.now() * 1000,
    resolveTimezone(value.timezone),
    value.time_format,
    true,
    value.date_format,
  );

  return (
    <Popover
      open={!disabled && open}
      onOpenChange={(next) => !disabled && setOpen(next)}
    >
      <DisabledControl
        disabled={disabled}
        reason={disabledReason}
        className="w-full"
      >
        <PopoverTrigger asChild>
          <button
            type="button"
            disabled={disabled}
            aria-disabled={disabled || undefined}
            aria-label={t('preferences.fields.date_time_format')}
            aria-expanded={open}
            className="flex h-9 w-full items-center justify-between gap-2 rounded-md border border-bd-1 bg-bg-2 px-3 font-sans text-sm text-tx-0 enabled:hover:border-bd-2 focus-visible:outline-none disabled:pointer-events-none disabled:cursor-not-allowed disabled:border-bd-0 disabled:bg-bg-3 disabled:text-tx-3"
          >
            <span className="truncate">
              {DATE_SAMPLES[value.date_format]} · {timeLabel}
            </span>
            <ChevronsUpDown className="h-4 w-4 shrink-0 text-tx-3" />
          </button>
        </PopoverTrigger>
      </DisabledControl>
      <PopoverContent
        align="end"
        className="w-[min(360px,calc(100vw-32px))] space-y-4 p-4"
      >
        <label className="block space-y-1.5">
          <span className="font-sans text-xs font-strong text-tx-1">
            {t('preferences.fields.date_format')}
          </span>
          <FormSelect
            value={value.date_format}
            onChange={(dateFormat) =>
              onChange({
                date_format: dateFormat as meApi.PreferenceDateFormat,
              })
            }
            ariaLabel={t('preferences.fields.date_format')}
            options={[
              {
                value: 'yyyy_mm_dd_dash',
                label: t('preferences.values.date_yyyy_mm_dd_dash'),
              },
              { value: 'yyyy_mm_dd_slash', label: '2026/07/27' },
              { value: 'dd_mm_yyyy_slash', label: '27/07/2026' },
              { value: 'mm_dd_yyyy_slash', label: '07/27/2026' },
            ]}
          />
        </label>
        <div className="space-y-1.5">
          <div className="font-sans text-xs font-strong text-tx-1">
            {t('preferences.fields.time_format')}
          </div>
          <SegmentedControl
            ariaLabel={t('preferences.fields.time_format')}
            value={value.time_format}
            onChange={(timeFormat) =>
              onChange({
                time_format: timeFormat as meApi.PreferenceTimeFormat,
              })
            }
            options={[
              {
                value: 'iso_24h',
                label: t('preferences.values.time_24h'),
              },
              {
                value: 'local_12h',
                label: t('preferences.values.time_12h'),
              },
            ]}
          />
        </div>
        <div className="rounded-md border border-bd-0 bg-bg-2 px-3 py-2.5">
          <div className="font-sans text-xs text-tx-3">
            {t('preferences.preview')}
          </div>
          <div className="mt-1 font-mono text-xs text-tx-0">{preview}</div>
        </div>
      </PopoverContent>
    </Popover>
  );
}

export function SegmentedControl<T extends string>({
  value,
  options,
  onChange,
  ariaLabel,
  disabled = false,
  disabledReason,
}: {
  value: T;
  options: Array<PreferenceOption<T>>;
  onChange: (value: T) => void;
  ariaLabel: string;
  disabled?: boolean;
  disabledReason?: string | undefined;
}) {
  return (
    <DisabledControl
      disabled={disabled}
      reason={disabledReason}
      className="w-full"
    >
      <div
        role="radiogroup"
        aria-label={ariaLabel}
        aria-disabled={disabled || undefined}
        className={cn(
          'grid min-h-9 w-full grid-flow-col auto-cols-fr gap-1 rounded-md border bg-bg-2 p-1',
          disabled ? 'border-bd-0 bg-bg-3' : 'border-bd-1',
        )}
      >
        {options.map((option) => {
          const selected = option.value === value;
          return (
            <button
              key={option.value}
              type="button"
              role="radio"
              disabled={disabled || option.disabled}
              aria-disabled={disabled || option.disabled || undefined}
              aria-checked={selected}
              onClick={() => onChange(option.value)}
              className={cn(
                'min-h-7 rounded-md px-2 font-sans text-xs font-strong transition-colors duration-fast focus-visible:outline-none disabled:pointer-events-none disabled:cursor-not-allowed disabled:text-tx-3',
                selected
                  ? 'bg-bg-1 text-tx-0 shadow-sm'
                  : 'text-tx-3 enabled:hover:bg-bg-3 enabled:hover:text-tx-1',
              )}
            >
              {option.label}
            </button>
          );
        })}
      </div>
    </DisabledControl>
  );
}

export function TimezoneCombobox({
  value,
  onChange,
  options,
  label,
  searchPlaceholder,
  emptyLabel,
  disabled = false,
  disabledReason,
}: {
  value: string;
  onChange: (value: string) => void;
  options: Array<PreferenceOption<string>>;
  label: string;
  searchPlaceholder: string;
  emptyLabel: string;
  disabled?: boolean;
  disabledReason?: string | undefined;
}) {
  const [open, setOpen] = React.useState(false);
  const selected = options.find((option) => option.value === value);

  return (
    <Popover
      open={!disabled && open}
      onOpenChange={(next) => !disabled && setOpen(next)}
    >
      <DisabledControl
        disabled={disabled}
        reason={disabledReason}
        className="w-full"
      >
        <PopoverTrigger asChild>
          <button
            type="button"
            role="combobox"
            disabled={disabled}
            aria-disabled={disabled || undefined}
            aria-label={label}
            aria-expanded={open}
            className="flex h-9 w-full min-w-0 items-center justify-between gap-2 rounded-md border border-bd-1 bg-bg-2 px-3 font-sans text-sm text-tx-0 transition-colors enabled:hover:border-bd-2 focus-visible:outline-none disabled:pointer-events-none disabled:cursor-not-allowed disabled:border-bd-0 disabled:bg-bg-3 disabled:text-tx-3"
          >
            <span className="truncate text-left">
              {selected?.label ?? value}
            </span>
            <ChevronsUpDown className="h-4 w-4 shrink-0 text-tx-3" />
          </button>
        </PopoverTrigger>
      </DisabledControl>
      <PopoverContent
        align="end"
        className="w-[var(--radix-popover-trigger-width)] p-0"
      >
        <Command>
          <CommandInput placeholder={searchPlaceholder} />
          <CommandList className="max-h-72">
            <CommandEmpty>{emptyLabel}</CommandEmpty>
            <CommandGroup>
              {options.map((option) => (
                <CommandItem
                  key={option.value}
                  value={`${option.label} ${option.value}`}
                  disabled={Boolean(option.disabled)}
                  aria-disabled={option.disabled || undefined}
                  {...(option.disabledReason
                    ? { title: option.disabledReason }
                    : {})}
                  onSelect={() => {
                    if (option.disabled) return;
                    onChange(option.value);
                    setOpen(false);
                  }}
                  className="min-h-9 data-[disabled=true]:cursor-not-allowed data-[disabled=true]:opacity-50"
                >
                  <Check
                    className={cn(
                      'h-4 w-4 shrink-0 text-indigo',
                      option.value === value ? 'opacity-100' : 'opacity-0',
                    )}
                  />
                  <span className="truncate">{option.label}</span>
                </CommandItem>
              ))}
            </CommandGroup>
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
}

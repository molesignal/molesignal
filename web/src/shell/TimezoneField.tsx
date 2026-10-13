import { Check, ChevronDown } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { buildTimezoneOptions } from '@/lib/time';
import { DisabledControl } from '@/shell/DisabledControl';
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

export interface TimezoneOption {
  value: string;
  label: string;
  disabled?: boolean;
  disabledReason?: string | undefined;
}

/**
 * A searchable list of time zones as a field: the control under every place
 * a zone is chosen in a form. What it lists is the caller's, so a form can put
 * an entry of its own first ("follow my browser").
 */
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
  options: readonly TimezoneOption[];
  label: string;
  searchPlaceholder: string;
  emptyLabel: string;
  disabled?: boolean;
  disabledReason?: string | undefined;
}) {
  const [open, setOpen] = React.useState(false);
  const selected = options.find((option) => option.value === value);

  return (
    <Popover open={!disabled && open} onOpenChange={(next) => !disabled && setOpen(next)}>
      <DisabledControl disabled={disabled} reason={disabledReason} className="w-full">
        <PopoverTrigger asChild>
          <button
            type="button"
            role="combobox"
            disabled={disabled}
            aria-disabled={disabled || undefined}
            aria-label={label}
            aria-expanded={open}
            className="flex h-9 w-full min-w-0 items-center justify-between gap-2 rounded-md border-0 bg-[var(--control-surface)] px-3 font-sans text-sm text-tx-0 transition-colors enabled:hover:bg-bg-3 data-[state=open]:bg-bg-3 focus-visible:outline-none disabled:pointer-events-none disabled:cursor-not-allowed disabled:bg-bg-3 disabled:text-tx-3"
          >
            <span className="truncate text-left">{selected?.label ?? value}</span>
            {/* The arrow of every other select in a form: a field is told by its fill and this
                mark, and that it can be searched is found on opening it. */}
            <ChevronDown aria-hidden="true" className="h-[14px] w-[14px] shrink-0 opacity-50" />
          </button>
        </PopoverTrigger>
      </DisabledControl>
      <PopoverContent align="end" className="w-[var(--radix-popover-trigger-width)] min-w-[280px] p-0">
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
                  {...(option.disabledReason ? { title: option.disabledReason } : {})}
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

/**
 * Every zone the browser knows, each with its offset now, `UTC` first. A zone
 * that is already set and is not among them (an alias the browser does not
 * list, a zone kept from an older server) stays in the list, so opening a form
 * never changes what it holds.
 */
export function timezoneFieldOptions(current: string): TimezoneOption[] {
  // The first entry of the shared list is "follow the browser", which a saved zone cannot be.
  const zones = buildTimezoneOptions('').slice(1);
  const known = zones.some((zone) => zone.value === current);

  return current && !known ? [{ value: current, label: current }, ...zones] : zones;
}

/**
 * One IANA time zone as a form field: a status page's, an on-call schedule's.
 * It is chosen from the list, never typed, so what is saved is always a zone
 * the server can resolve.
 */
export function TimezoneField({
  value,
  onChange,
  label,
  disabled,
  disabledReason,
}: {
  value: string;
  onChange: (timezone: string) => void;
  /** The field's name for assistive technology, when its label is not a `<label>` around it. */
  label: string;
  disabled?: boolean;
  disabledReason?: string | undefined;
}) {
  const { t } = useTranslation('common');
  const options = React.useMemo(() => timezoneFieldOptions(value), [value]);

  return (
    <TimezoneCombobox
      value={value}
      onChange={onChange}
      options={options}
      label={label}
      searchPlaceholder={t('timezone.search')}
      emptyLabel={t('timezone.empty')}
      {...(disabled !== undefined ? { disabled } : {})}
      disabledReason={disabledReason}
    />
  );
}

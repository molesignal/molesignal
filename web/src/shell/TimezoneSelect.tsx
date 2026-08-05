import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Check, ChevronsUpDown } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as meApi from '@/api/me';
import { toApiError } from '@/lib/http';
import {
  buildTimezoneOptions,
  FOLLOW_TZ_SENTINEL,
  resolveTimezone,
  tzOffsetLabel,
} from '@/lib/time';
import { cn } from '@/shell/lib/cn';
import {
  USER_PREFERENCES_QUERY_KEY,
  useApplyUserPreferences,
} from '@/shell/PreferenceRuntime';
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
import { toast } from '@/shell/ui/sonner';

const BROWSER_OVERRIDE = '__browser_override__';

/**
 * Per-page timezone override. An empty value means "use my default"; choosing
 * an explicit zone affects only the mounted page. The footer action promotes
 * that temporary choice to the shared personal default.
 */
export function TimezoneSelect({
  value,
  onChange,
  className,
}: {
  value: string;
  onChange: (timezone: string) => void;
  className?: string;
}) {
  const { t } = useTranslation('common');
  const [open, setOpen] = React.useState(false);
  const queryClient = useQueryClient();
  const applyUserPreferences = useApplyUserPreferences();
  const preferencesQuery = useQuery({
    queryKey: USER_PREFERENCES_QUERY_KEY,
    queryFn: () => meApi.preferences(),
    staleTime: 5 * 60_000,
  });
  const preferences =
    preferencesQuery.data ?? meApi.DEFAULT_USER_PREFERENCES;
  const browserTimezone = resolveTimezone('');
  const defaultTimezone = resolveTimezone(preferences.timezone);
  const defaultOffset = tzOffsetLabel(defaultTimezone);
  const overrideOffset = value ? tzOffsetLabel(value) : '';
  const options = React.useMemo(
    () => [
      {
        value: FOLLOW_TZ_SENTINEL,
        label: t('timezone.use_personal_default_option', {
          timezone: defaultTimezone,
          offset: defaultOffset,
        }),
      },
      {
        value: BROWSER_OVERRIDE,
        label: t('timezone.browser_option', {
          timezone: browserTimezone,
          offset: tzOffsetLabel(browserTimezone),
        }),
      },
      ...buildTimezoneOptions(t('timezone.browser')).slice(1),
    ],
    [browserTimezone, defaultOffset, defaultTimezone, t],
  );

  const saveDefault = useMutation({
    mutationFn: () =>
      meApi.updatePreferences({ ...preferences, timezone: value }),
    onSuccess: (saved) => {
      queryClient.setQueryData(USER_PREFERENCES_QUERY_KEY, saved);
      applyUserPreferences(saved);
      onChange('');
      setOpen(false);
      toast.success(t('timezone.default_saved'));
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const triggerLabel = value
    ? t('timezone.page_override_label', {
        timezone: value,
        offset: overrideOffset,
      })
    : t('timezone.use_personal_default', { offset: defaultOffset });

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button
          type="button"
          role="combobox"
          aria-label={t('timezone.label')}
          aria-expanded={open}
          className={cn(
            'flex h-9 min-w-0 max-w-[280px] items-center justify-between gap-2 rounded-md border border-bd-1 bg-bg-1 px-2.5 font-sans text-xs font-strong text-tx-1 hover:border-bd-2 focus-visible:outline-none',
            className,
          )}
        >
          <span className="truncate">{triggerLabel}</span>
          <ChevronsUpDown className="h-3.5 w-3.5 shrink-0 text-tx-3" />
        </button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-[340px] max-w-[calc(100vw-32px)] p-0">
        <Command>
          <CommandInput placeholder={t('timezone.search')} />
          <CommandList className="max-h-72">
            <CommandEmpty>{t('timezone.empty')}</CommandEmpty>
            <CommandGroup>
              {options.map((option) => {
                const selected =
                  option.value === FOLLOW_TZ_SENTINEL
                    ? !value
                    : option.value === BROWSER_OVERRIDE
                      ? value === browserTimezone
                      : value === option.value;
                return (
                  <CommandItem
                    key={option.value}
                    value={`${option.label} ${option.value}`}
                    onSelect={() => {
                      if (option.value === FOLLOW_TZ_SENTINEL) onChange('');
                      else if (option.value === BROWSER_OVERRIDE) {
                        onChange(browserTimezone);
                      } else {
                        onChange(option.value);
                      }
                      setOpen(false);
                    }}
                    className="min-h-9"
                  >
                    <Check
                      className={cn(
                        'h-4 w-4 shrink-0 text-indigo',
                        selected ? 'opacity-100' : 'opacity-0',
                      )}
                    />
                    <span className="truncate">{option.label}</span>
                  </CommandItem>
                );
              })}
            </CommandGroup>
          </CommandList>
        </Command>
        {value && (
          <div className="border-t border-bd-0 p-2">
            <button
              type="button"
              disabled={saveDefault.isPending}
              onClick={() => saveDefault.mutate()}
              className="flex min-h-9 w-full items-center rounded-md px-2.5 text-left font-sans text-xs font-strong text-indigo-soft hover:bg-bg-3 disabled:cursor-not-allowed disabled:opacity-50"
            >
              {saveDefault.isPending
                ? t('timezone.saving_default')
                : t('timezone.set_as_default')}
            </button>
          </div>
        )}
      </PopoverContent>
    </Popover>
  );
}

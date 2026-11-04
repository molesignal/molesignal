import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { Check, Globe } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as meApi from '@/api/me';
import { toApiError } from '@/lib/http';
import { activeTimeZone, buildTimezoneOptions, resolveTimezone, setActiveTimePreference, tzOffsetLabel } from '@/lib/time';
import { cn } from '@/shell/lib/cn';
import { USER_PREFERENCES_QUERY_KEY } from '@/shell/PreferenceRuntime';
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from '@/shell/ui/command';
import { toast } from '@/shell/ui/sonner';
import { useTimeStore } from '@/stores/useTimeStore';

const PERSONAL_DEFAULT = '__personal_default__';
const BROWSER = '__browser__';

/** A zone as the picker says it: its name and its offset, or its name alone where the two are one (UTC). */
export function zoneText(timezone: string): string {
  const offset = tzOffsetLabel(timezone);
  return offset === timezone ? timezone : `${timezone} · ${offset}`;
}

/**
 * The zone the times in scope are shown in: the one picked in the time picker,
 * or the user's default as the shell applied it. Read without asking for the
 * preferences, so a picker stands anywhere a time range does.
 */
export function useEffectiveTimezone(): { timezone: string; picked: boolean } {
  const picked = useTimeStore((state) => state.timezone);
  return { timezone: picked ? resolveTimezone(picked) : activeTimeZone(), picked: picked !== '' };
}

/**
 * The foot of the time picker, as Grafana has it: the zone the times in scope
 * are shown in, the user's default until another is picked. A picked zone
 * holds for the time in scope only (this page, or this query tab), and can be
 * made the user's default from here.
 */
export function TimezoneSection() {
  const { t } = useTranslation('common');
  const timezone = useTimeStore((state) => state.timezone);
  const setTimezone = useTimeStore((state) => state.setTimezone);
  const [choosing, setChoosing] = React.useState(false);
  const queryClient = useQueryClient();
  const preferencesQuery = useQuery({
    queryKey: USER_PREFERENCES_QUERY_KEY,
    queryFn: () => meApi.preferences(),
    staleTime: 5 * 60_000,
  });
  const preferences = preferencesQuery.data ?? meApi.DEFAULT_USER_PREFERENCES;
  const browserTimezone = resolveTimezone('');
  const defaultTimezone = resolveTimezone(preferences.timezone);

  const options = React.useMemo(
    () => [
      {
        value: PERSONAL_DEFAULT,
        label: t('timezone.use_personal_default_option', { zone: zoneText(defaultTimezone) }),
      },
      {
        value: BROWSER,
        label: t('timezone.browser_option', { zone: zoneText(browserTimezone) }),
      },
      ...buildTimezoneOptions(t('timezone.browser')).slice(1),
    ],
    [browserTimezone, defaultTimezone, t],
  );

  const saveDefault = useMutation({
    mutationFn: () => meApi.updatePreferences({ ...preferences, timezone }),
    onSuccess: (saved) => {
      queryClient.setQueryData(USER_PREFERENCES_QUERY_KEY, saved);
      // Only the zone changed: the times formatted outside React follow it, the rest of the shell stays as it is.
      setActiveTimePreference(saved.timezone, saved.timeFormat, saved.dateFormat);
      setTimezone('');
      toast.success(t('timezone.default_saved'));
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const current = timezone
    ? t('timezone.page_override_label', { zone: zoneText(timezone) })
    : t('timezone.use_personal_default_option', { zone: zoneText(defaultTimezone) });

  const isSelected = (value: string) =>
    value === PERSONAL_DEFAULT ? !timezone : value === BROWSER ? timezone === browserTimezone : timezone === value;

  return (
    <section data-time-picker-timezone className="border-t border-bd-0 px-3 py-2">
      <div className="flex min-h-8 items-center gap-2 font-sans text-xs">
        <Globe aria-hidden="true" className="h-3.5 w-3.5 shrink-0 text-tx-3" />
        <span className="shrink-0 font-strong text-tx-2">{t('timezone.label')}</span>
        <span className="min-w-0 flex-1 truncate text-tx-1" data-time-picker-timezone-current title={current}>
          {current}
        </span>
        {timezone && (
          <button
            type="button"
            disabled={saveDefault.isPending}
            onClick={() => saveDefault.mutate()}
            className="shrink-0 rounded-md px-2 py-1 font-strong text-indigo-soft hover:bg-bg-3 focus-visible:bg-bg-3 focus-visible:outline-none disabled:cursor-not-allowed disabled:opacity-50"
          >
            {saveDefault.isPending ? t('timezone.saving_default') : t('timezone.set_as_default')}
          </button>
        )}
        <button
          type="button"
          aria-expanded={choosing}
          onClick={() => setChoosing((open) => !open)}
          className="shrink-0 rounded-md bg-[var(--control-surface)] px-2.5 py-1 font-strong text-tx-1 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0 focus-visible:outline-none"
        >
          {t(choosing ? 'timezone.done' : 'timezone.change')}
        </button>
      </div>
      {choosing && (
        <Command className="mt-2 rounded-md bg-[var(--control-surface)]">
          <CommandInput placeholder={t('timezone.search')} autoFocus />
          <CommandList className="max-h-56">
            <CommandEmpty>{t('timezone.empty')}</CommandEmpty>
            <CommandGroup>
              {options.map((option) => (
                <CommandItem
                  key={option.value}
                  value={`${option.label} ${option.value}`}
                  onSelect={() => {
                    setTimezone(option.value === PERSONAL_DEFAULT ? '' : option.value === BROWSER ? browserTimezone : option.value);
                    setChoosing(false);
                  }}
                  className="min-h-8"
                >
                  <Check aria-hidden="true" className={cn('h-4 w-4 shrink-0 text-indigo', isSelected(option.value) ? 'opacity-100' : 'opacity-0')} />
                  <span className="truncate">{option.label}</span>
                </CommandItem>
              ))}
            </CommandGroup>
          </CommandList>
        </Command>
      )}
    </section>
  );
}

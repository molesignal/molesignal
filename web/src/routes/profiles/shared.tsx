import { useTranslation } from 'react-i18next';

import type { ProfileTypeName } from '@/api/profiles';
import { cn } from '@/shell/lib/cn';
import {
  FITTED_SELECT_SEPARATOR,
  FittedSelect,
  type FittedSelectEntry,
} from '@/shell/ui/fitted-select';
import { resolveWindow, type TimeWindow, useTimeStore } from '@/stores/useTimeStore';

/** Sentinel for "all" — Radix Select forbids an empty-string item value. */
const ALL = '__all__';

/**
 * The widest a filter of the profile pages gets. Each is as wide as its longest
 * choice up to this, and a name longer than it is cut and said under the pointer.
 */
const SELECT_MAX_WIDTH = 240;

export const PROFILE_TYPE_KEYS: ProfileTypeName[] = [
  'cpu',
  'wall',
  'alloc_space',
  'alloc_objects',
  'inuse_space',
  'inuse_objects',
  'goroutines',
  'lock',
];

/** Resolve the global time window into a stable {from,to} micros pair plus the
 *  raw expr strings (useful as react-query key segments). */
export function useWindowMicros(): {
  window: TimeWindow;
  fromMicros: number;
  toMicros: number;
} {
  const window = useTimeStore((s) => s.window);
  const { from, to } = resolveWindow(window);
  return { window, fromMicros: from.getTime() * 1000, toMicros: to.getTime() * 1000 };
}

/**
 * The types a list of profiles has, in the order the types are offered in:
 * the first of them is the one a page opens on.
 */
export function profileTypesIn(rows: readonly { profile_type: string }[]): string[] {
  const present = new Set(rows.map((row) => row.profile_type));
  const known = (PROFILE_TYPE_KEYS as string[]).filter((key) => present.has(key));

  return [...known, ...[...present].filter((type) => !known.includes(type)).sort()];
}

/** Where the analysis of a scope is compared with a baseline: the scope goes along. */
export function compareHref(scope: { service?: string | undefined; type?: string | undefined }): string {
  const params = new URLSearchParams();
  if (scope.service) params.set('service', scope.service);
  if (scope.type) params.set('type', scope.type);
  const query = params.toString();

  return query ? `/profiles/compare?${query}` : '/profiles/compare';
}

/**
 * Which profile type is looked at. There is no "all": a flame graph adds up
 * values, and seconds of CPU do not add up with bytes of memory.
 */
export function TypeSelect({
  value,
  onChange,
  present,
}: {
  value: string;
  onChange: (v: string) => void;
  /**
   * The types there are profiles of, where that is known. They are offered
   * first; the types nothing was captured of come after them, muted.
   */
  present?: readonly string[] | undefined;
}) {
  const { t } = useTranslation('profiles');
  const label = useProfileTypeLabel();
  const known = PROFILE_TYPE_KEYS as string[];
  const first = present ? [...present] : [...known];
  const rest = present ? known.filter((key) => !present.includes(key)) : [];
  // The type looked at is always one of the choices, though nothing may have it.
  if (value && !first.includes(value) && !rest.includes(value)) first.push(value);

  const options: FittedSelectEntry[] = [
    ...first.map((key) => ({ value: key, label: label(key) })),
    ...(first.length > 0 && rest.length > 0 ? [FITTED_SELECT_SEPARATOR] as const : []),
    ...rest.map((key) => ({ value: key, label: label(key), muted: true })),
  ];

  return (
    <FittedSelect
      data-profile-type-select
      value={value}
      onValueChange={onChange}
      options={options}
      maxWidth={SELECT_MAX_WIDTH}
      placeholder={t('filters.type')}
      aria-label={t('filters.type')}
    />
  );
}

/** Which service is looked at, or all of them. */
export function ServiceSelect({
  value,
  services,
  onChange,
}: {
  value: string;
  services: string[];
  onChange: (v: string) => void;
}) {
  const { t } = useTranslation('profiles');
  // The service looked at stays a choice though the period has nothing of it.
  const names = value && !services.includes(value) ? [...services, value] : services;

  return (
    <FittedSelect
      data-profile-service-select
      value={value || ALL}
      onValueChange={(next) => onChange(next === ALL ? '' : next)}
      options={[
        { value: ALL, label: t('filters.service_all') },
        ...names.map((name) => ({ value: name, label: name })),
      ]}
      maxWidth={SELECT_MAX_WIDTH}
      aria-label={t('filters.service')}
    />
  );
}

/** Localized label for a profile type, falling back to the raw type name. */
export function useProfileTypeLabel(): (type: string) => string {
  const { t } = useTranslation('profiles');
  return (type: string) =>
    (PROFILE_TYPE_KEYS as string[]).includes(type) ? t(`profile_types.${type}`) : type;
}

/** Non-blocking notice shown when the merge sampled the window. */
export function TruncatedNotice({ className }: { className?: string | undefined }) {
  const { t } = useTranslation('profiles');
  return (
    <div
      className={cn(
        'rounded-md border border-yellow/30 bg-[var(--yellow-tint)] px-3 py-2 font-sans text-xs text-[var(--yellow-ink)]',
        className,
      )}
      role="status"
    >
      {t('flamegraph.truncated_notice')}
    </div>
  );
}

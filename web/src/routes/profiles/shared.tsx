import { useTranslation } from 'react-i18next';

import type { ProfileTypeName } from '@/api/profiles';
import { cn } from '@/shell/lib/cn';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/shell/ui/select';
import { resolveWindow, type TimeWindow, useTimeStore } from '@/stores/useTimeStore';

/** Sentinel for "all" — Radix Select forbids an empty-string item value. */
const ALL = '__all__';

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

interface RangePreset {
  expr: string;
  key: string;
}

export const RANGE_PRESETS: RangePreset[] = [
  { expr: 'now-15m', key: 'm15' },
  { expr: 'now-1h', key: 'h1' },
  { expr: 'now-6h', key: 'h6' },
  { expr: 'now-24h', key: 'h24' },
  { expr: 'now-7d', key: 'd7' },
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

/** A standalone duration select (not bound to the global time store). Used by
 *  the compare view to pick the period it diffs against the preceding one. */
export function DurationSelect({
  value,
  onChange,
  ariaLabel,
}: {
  value: string;
  onChange: (v: string) => void;
  ariaLabel?: string | undefined;
}) {
  const { t } = useTranslation('profiles');
  return (
    <Select value={value} onValueChange={onChange}>
      <SelectTrigger className="h-8 w-[150px] font-sans text-xs" aria-label={ariaLabel}>
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        {RANGE_PRESETS.map((p) => (
          <SelectItem key={p.expr} value={p.expr} className="text-xs">
            {t(`filters.ranges.${p.key}`)}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}

export function TypeSelect({
  value,
  onChange,
}: {
  value: string;
  onChange: (v: string) => void;
}) {
  const { t } = useTranslation('profiles');
  return (
    <Select value={value || ALL} onValueChange={(v) => onChange(v === ALL ? '' : v)}>
      <SelectTrigger className="h-8 w-[160px] font-sans text-xs" aria-label={t('filters.type')}>
        <SelectValue placeholder={t('filters.type')} />
      </SelectTrigger>
      <SelectContent>
        <SelectItem value={ALL} className="text-xs">
          {t('filters.type_all')}
        </SelectItem>
        {PROFILE_TYPE_KEYS.map((key) => (
          <SelectItem key={key} value={key} className="text-xs">
            {t(`profile_types.${key}`)}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}

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
  return (
    <Select value={value || ALL} onValueChange={(v) => onChange(v === ALL ? '' : v)}>
      <SelectTrigger className="h-8 w-[180px] font-sans text-xs" aria-label={t('filters.service')}>
        <SelectValue placeholder={t('filters.service')} />
      </SelectTrigger>
      <SelectContent>
        <SelectItem value={ALL} className="text-xs">
          {t('filters.service_all')}
        </SelectItem>
        {services.map((svc) => (
          <SelectItem key={svc} value={svc} className="text-xs">
            {svc}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
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
        'rounded-md border border-yellow/30 bg-yellow-dim px-3 py-2 font-sans text-xs text-yellow-soft',
        className,
      )}
      role="status"
    >
      {t('flamegraph.truncated_notice')}
    </div>
  );
}

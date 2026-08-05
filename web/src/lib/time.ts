import { useQuery } from '@tanstack/react-query';
import dayjs from 'dayjs';
import timezonePlugin from 'dayjs/plugin/timezone';
import utcPlugin from 'dayjs/plugin/utc';

import * as meApi from '@/api/me';

dayjs.extend(utcPlugin);
dayjs.extend(timezonePlugin);

export type TimeFormat = meApi.PreferenceTimeFormat; // 'iso_24h' | 'local_12h'
export type DateFormat = meApi.PreferenceDateFormat;

const DATE_PATTERNS: Record<DateFormat, string> = {
  yyyy_mm_dd_dash: 'YYYY-MM-DD',
  yyyy_mm_dd_slash: 'YYYY/MM/DD',
  dd_mm_yyyy_slash: 'DD/MM/YYYY',
  mm_dd_yyyy_slash: 'MM/DD/YYYY',
};

/** 全量 IANA 时区名（浏览器提供）；时区选择器选项用。cast 兜底 TS lib 未声明该 API。 */
export const TIMEZONE_NAMES: string[] =
  (Intl as { supportedValuesOf?: (key: 'timeZone') => string[] }).supportedValuesOf?.('timeZone') ??
  [];

/**
 * 中央时间渲染层。
 *
 * 后端一律按 epoch 微秒(UTC) 存储；这里统一把瞬时按「用户偏好时区」+「时间格式」渲染。
 * 偏好 timezone 为空串表示「跟随浏览器」，解析为浏览器本地时区；各视图可传 override 覆盖。
 */

/** 解析有效时区：空串/未设 → 浏览器本地时区（`dayjs.tz.guess()`）。 */
export function resolveTimezone(tz: string | undefined | null): string {
  const trimmed = (tz ?? '').trim();
  return trimmed || dayjs.tz.guess();
}

/** 把 epoch 微秒按给定时区 + 格式渲染。空/0 → `—`。 */
export function formatMicros(
  micros: number | null | undefined,
  tz: string,
  format: TimeFormat,
  withSeconds = true,
  dateFormat: DateFormat = 'yyyy_mm_dd_dash',
): string {
  if (micros == null || micros === 0) return '—';
  const instant = dayjs(micros / 1000).tz(tz);
  if (!instant.isValid()) return '—';
  const datePattern = DATE_PATTERNS[dateFormat];
  if (format === 'local_12h') {
    return instant.format(
      withSeconds
        ? `${datePattern} hh:mm:ss A`
        : `${datePattern} hh:mm A`,
    );
  }
  return instant.format(
    withSeconds ? `${datePattern} HH:mm:ss` : `${datePattern} HH:mm`,
  );
}

/**
 * 时区选择器里「跟随全局 / 浏览器」选项的哨兵值。radix `Select.Item` 禁止空串 value，
 * 故选项用非空哨兵，读写时映射回空串（空串语义=跟随偏好 / 浏览器）。
 */
export const FOLLOW_TZ_SENTINEL = '__follow__';

// ── 模块级「当前生效偏好」：供散落各处的非 hook 格式化（如 rum/_helpers.formatMicros）复用，
// 由 ShellRoot 偏好加载 effect 与偏好设置保存时调用 setActiveTimePreference 注入。
let activeTimezone = '';
let activeFormat: TimeFormat = 'iso_24h';
let activeDateFormat: DateFormat = 'yyyy_mm_dd_dash';

/** 注入当前生效的全局时区 / 时间格式偏好（空时区=跟随浏览器）。 */
export function setActiveTimePreference(
  timezone: string,
  format: TimeFormat,
  dateFormat: DateFormat = 'yyyy_mm_dd_dash',
): void {
  activeTimezone = timezone;
  activeFormat = format;
  activeDateFormat = dateFormat;
}

/** 用当前生效的全局偏好渲染 epoch 微秒（非 hook，给散落的纯函数 helper 复用）。 */
export function formatMicrosActive(micros: number | null | undefined, withSeconds = true): string {
  return formatMicros(
    micros,
    resolveTimezone(activeTimezone),
    activeFormat,
    withSeconds,
    activeDateFormat,
  );
}

/** 当前生效的全局时区（解析后；空=浏览器本地）。供非 hook 的图表轴等纯函数复用。 */
export function activeTimeZone(): string {
  return resolveTimezone(activeTimezone);
}

/** 当前生效的全局时间格式（12h / 24h）。 */
export function activeTimeFormat(): TimeFormat {
  return activeFormat;
}

/** 某 IANA 时区当前的 UTC 偏移标签，如 `UTC+8` / `UTC-5` / `UTC+5:30`（偏移为零时回退 `UTC`）。 */
export function tzOffsetLabel(timezone: string): string {
  try {
    const parts = new Intl.DateTimeFormat('en-US', {
      timeZone: timezone,
      timeZoneName: 'shortOffset',
    }).formatToParts(new Date());
    const name = parts.find((part) => part.type === 'timeZoneName')?.value ?? 'UTC';
    const label = name.replace('GMT', 'UTC');
    return /^UTC(?:[+-]0(?::?00)?)?$/.test(label) ? 'UTC' : label;
  } catch {
    return 'UTC';
  }
}

// IANA 时区选项（含当前 UTC 偏移） - 模块加载时一次性计算，避免每次渲染重复构造 Intl。
const ZONE_OPTIONS: ReadonlyArray<{ value: string; label: string }> = TIMEZONE_NAMES
  .filter((tz) => tz !== 'UTC' && tz !== 'Etc/UTC')
  .map((tz) => ({ value: tz, label: `${tz} · ${tzOffsetLabel(tz)}` }));

/**
 * 时区下拉选项：首项「跟随/浏览器」（带当前浏览器偏移），次项显式 `UTC`，其余为全量 IANA
 * 时区（各带当前 UTC 偏移）。`followLabel` 由调用方给出语义文案。
 */
export function buildTimezoneOptions(followLabel: string): Array<{ value: string; label: string }> {
  const browserTimezone = resolveTimezone('');
  return [
    {
      value: FOLLOW_TZ_SENTINEL,
      label: `${followLabel} · ${browserTimezone} · ${tzOffsetLabel(browserTimezone)}`,
    },
    { value: 'UTC', label: 'UTC' },
    ...ZONE_OPTIONS,
  ];
}

export interface TimeFormatterOverride {
  /** 覆盖时区（per-view 时区选择器用）；省略 / undefined 则用偏好 / 浏览器时区。 */
  timezone?: string | undefined;
  /** 覆盖时间格式；省略 / undefined 则用偏好。 */
  format?: TimeFormat | undefined;
}

export interface TimeFormatter {
  /** 生效时区（IANA 名）。 */
  tz: string;
  /** 生效时间格式。 */
  format: TimeFormat;
  /** 生效日期格式。 */
  dateFormat: DateFormat;
  /** 渲染 epoch 微秒。 */
  micros: (micros: number | null | undefined, withSeconds?: boolean) => string;
  /** 渲染 epoch 毫秒。 */
  millis: (millis: number | null | undefined, withSeconds?: boolean) => string;
  /** 渲染 ISO / Date 可解析的时间串。 */
  iso: (value: string | number | Date | null | undefined, withSeconds?: boolean) => string;
}

/**
 * Hook：读取用户偏好（timezone + time_format）并返回绑定好的格式化器。
 * 传 `override.timezone` 即可在单个视图覆盖全局偏好（per-view 时区选择器）。
 */
export function useTimeFormatter(override?: TimeFormatterOverride): TimeFormatter {
  const { data } = useQuery({
    queryKey: ['me', 'preferences'],
    queryFn: () => meApi.preferences(),
    staleTime: 5 * 60_000,
  });
  const prefs = data ?? meApi.DEFAULT_USER_PREFERENCES;
  const tz = resolveTimezone(override?.timezone ?? prefs.timezone);
  const format = override?.format ?? prefs.time_format;
  const dateFormat = prefs.date_format;

  return {
    tz,
    format,
    dateFormat,
    micros: (micros, withSeconds = true) =>
      formatMicros(micros, tz, format, withSeconds, dateFormat),
    millis: (millis, withSeconds = true) =>
      formatMicros(
        millis == null ? millis : millis * 1000,
        tz,
        format,
        withSeconds,
        dateFormat,
      ),
    iso: (value, withSeconds = true) => {
      if (value == null || value === '') return '—';
      const ms = dayjs(value).valueOf();
      return formatMicros(
        Number.isNaN(ms) ? null : ms * 1000,
        tz,
        format,
        withSeconds,
        dateFormat,
      );
    },
  };
}

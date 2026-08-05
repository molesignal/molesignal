import { activeTimeZone } from '@/lib/time';

const ISO_DATE_TIME_LOCALE = 'en-CA-u-ca-iso8601-nu-latn';
const isoDateTimeFormatters = new Map<string, Intl.DateTimeFormat>();

interface IsoDateTimeParts {
  year: string;
  month: string;
  day: string;
  hour: string;
  minute: string;
  second: string;
  fractionalSecond: string;
}

export function formatTimeSeriesValue(value: number, unit?: string): string {
  if (!Number.isFinite(value)) return '—';
  const normalizedUnit = unit?.trim().toLowerCase();
  if (normalizedUnit && ['bytes', 'byte', 'decbytes', 'binb'].includes(normalizedUnit)) {
    return formatBytes(value);
  }
  if (normalizedUnit && ['ns', 'nanoseconds'].includes(normalizedUnit)) {
    return formatDuration(value / 1_000_000_000);
  }
  if (normalizedUnit && ['us', 'µs', 'μs', 'microseconds'].includes(normalizedUnit)) {
    return formatDuration(value / 1_000_000);
  }
  if (normalizedUnit && ['ms', 'milliseconds'].includes(normalizedUnit)) {
    return formatDuration(value / 1000);
  }
  if (normalizedUnit && ['s', 'sec', 'seconds'].includes(normalizedUnit)) {
    return formatDuration(value);
  }
  if (normalizedUnit && ['percent', 'percentunit', '%'].includes(normalizedUnit)) {
    const percent = normalizedUnit === 'percentunit' ? value * 100 : value;
    return `${formatCompactNumber(percent)}%`;
  }
  const suffix = unit && !['short', 'none'].includes(normalizedUnit ?? '') ? ` ${unit}` : '';
  return `${formatCompactNumber(value)}${suffix}`;
}

export function formatCompactNumber(value: number): string {
  if (!Number.isFinite(value)) return '—';
  const abs = Math.abs(value);
  if (abs === 0) return '0';
  if (abs >= 1e12) return `${trimFixed(value / 1e12, 2)}T`;
  if (abs >= 1e9) return `${trimFixed(value / 1e9, 2)}G`;
  if (abs >= 1e6) return `${trimFixed(value / 1e6, 2)}M`;
  if (abs >= 1e3) return `${trimFixed(value / 1e3, 2)}K`;
  if (abs >= 100) return trimFixed(value, 0);
  if (abs >= 10) return trimFixed(value, 1);
  if (abs >= 1) return trimFixed(value, 2);
  if (abs < 0.001) return value.toExponential(2);
  return trimFixed(value, 3);
}

export function formatTimeSeriesTimestamp(
  epochSeconds: number,
  full = false,
  timezone = activeTimeZone(),
): string {
  if (!Number.isFinite(epochSeconds)) return '—';
  const parts = isoDateTimeParts(epochSeconds, timezone);
  if (!parts) return '—';
  const time = `${parts.hour}:${parts.minute}:${parts.second}`;
  return full
    ? `${parts.year}-${parts.month}-${parts.day} ${time}.${parts.fractionalSecond}`
    : `${parts.month}-${parts.day} ${time}`;
}

export function formatTimeSeriesAxisTimestamp(
  epochSeconds: number,
  visibleSpanSeconds: number,
  timezone = activeTimeZone(),
): string {
  if (!Number.isFinite(epochSeconds)) return '';
  const parts = isoDateTimeParts(epochSeconds, timezone);
  if (!parts) return '';
  if (visibleSpanSeconds > 31 * 86_400) {
    return `${parts.year}-${parts.month}-${parts.day}`;
  }
  if (visibleSpanSeconds > 7 * 86_400) {
    return `${parts.month}-${parts.day}`;
  }
  if (visibleSpanSeconds > 86_400) {
    return `${parts.month}-${parts.day} ${parts.hour}:${parts.minute}`;
  }
  if (visibleSpanSeconds <= 3600) {
    return `${parts.hour}:${parts.minute}:${parts.second}`;
  }
  return `${parts.hour}:${parts.minute}`;
}

function isoDateTimeParts(
  epochSeconds: number,
  timezone: string,
): IsoDateTimeParts | null {
  const date = new Date(epochSeconds * 1000);
  if (Number.isNaN(date.getTime())) return null;
  try {
    let formatter = isoDateTimeFormatters.get(timezone);
    if (!formatter) {
      formatter = new Intl.DateTimeFormat(ISO_DATE_TIME_LOCALE, {
        calendar: 'iso8601',
        numberingSystem: 'latn',
        timeZone: timezone,
        year: 'numeric',
        month: '2-digit',
        day: '2-digit',
        hour: '2-digit',
        minute: '2-digit',
        second: '2-digit',
        fractionalSecondDigits: 3,
        hourCycle: 'h23',
      });
      isoDateTimeFormatters.set(timezone, formatter);
    }
    const values = Object.fromEntries(
      formatter
        .formatToParts(date)
        .filter((part) => part.type !== 'literal')
        .map((part) => [part.type, part.value]),
    ) as Partial<IsoDateTimeParts>;
    if (
      !values.year ||
      !values.month ||
      !values.day ||
      !values.hour ||
      !values.minute ||
      !values.second
    ) {
      return null;
    }
    return {
      year: values.year,
      month: values.month,
      day: values.day,
      hour: values.hour,
      minute: values.minute,
      second: values.second,
      fractionalSecond: values.fractionalSecond ?? '000',
    };
  } catch {
    return null;
  }
}

function formatBytes(value: number): string {
  const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB', 'PiB'];
  const sign = value < 0 ? -1 : 1;
  let scaled = Math.abs(value);
  let unitIndex = 0;
  while (scaled >= 1024 && unitIndex < units.length - 1) {
    scaled /= 1024;
    unitIndex += 1;
  }
  return `${trimFixed(scaled * sign, scaled >= 100 ? 0 : scaled >= 10 ? 1 : 2)} ${units[unitIndex]}`;
}

function formatDuration(seconds: number): string {
  const abs = Math.abs(seconds);
  if (abs >= 1) return `${trimFixed(seconds, abs >= 100 ? 0 : 2)} s`;
  if (abs >= 0.001) return `${trimFixed(seconds * 1000, 2)} ms`;
  if (abs >= 0.000001) return `${trimFixed(seconds * 1_000_000, 2)} μs`;
  return `${trimFixed(seconds * 1_000_000_000, 2)} ns`;
}

function trimFixed(value: number, decimals: number): string {
  return value
    .toFixed(decimals)
    .replace(/\.0+$/, '')
    .replace(/(\.\d*?)0+$/, '$1');
}

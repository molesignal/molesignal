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
  // A zero has no size to pick a unit by: it keeps the unit the value is in, so that the first tick
  // of an axis in milliseconds reads "0 ms" and not "0 ns".
  if (normalizedUnit && ['ns', 'nanoseconds'].includes(normalizedUnit)) {
    return value === 0 ? '0 ns' : formatDuration(value / 1_000_000_000);
  }
  if (normalizedUnit && ['us', 'µs', 'μs', 'microseconds'].includes(normalizedUnit)) {
    return value === 0 ? '0 μs' : formatDuration(value / 1_000_000);
  }
  if (normalizedUnit && ['ms', 'milliseconds'].includes(normalizedUnit)) {
    return value === 0 ? '0 ms' : formatDuration(value / 1000);
  }
  if (normalizedUnit && ['s', 'sec', 'seconds'].includes(normalizedUnit)) {
    return value === 0 ? '0 s' : formatDuration(value);
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

/**
 * The figures of a value axis, all in one unit: the unit the largest tick reads best in, written once,
 * on that tick. A zero is a bare `0`. So an axis in microseconds reads `0 / 250 / 500 / 750 ms`, not
 * `0 μs / 250 ms / …`, and a rate does not repeat `req/s` on every line.
 */
export function formatTimeSeriesAxisValues(values: readonly number[], unit?: string): string[] {
  const finite = values.filter((value) => Number.isFinite(value));
  if (finite.length === 0) return values.map(() => '');
  const top = Math.max(...finite);
  const largest = Math.max(...finite.map((value) => Math.abs(value)));
  const scale = axisScale(largest, unit);
  const step = axisStep(finite.map((value) => value * scale.factor));

  return values.map((value) => {
    if (!Number.isFinite(value)) return '';
    if (value === 0) return '0';
    const figure = trimFixed(value * scale.factor, step);
    return value === top && scale.label ? `${figure}${scale.label}` : figure;
  });
}

interface AxisScale {
  /** What a value is multiplied by to be written. */
  factor: number;
  /** What follows the top figure, its space included; empty for a count with no unit. */
  label: string;
}

const DURATION_SECONDS: Record<string, number> = {
  ns: 1e-9,
  nanoseconds: 1e-9,
  us: 1e-6,
  'µs': 1e-6,
  'μs': 1e-6,
  microseconds: 1e-6,
  ms: 1e-3,
  milliseconds: 1e-3,
  s: 1,
  sec: 1,
  seconds: 1,
};

const DURATION_STEPS: ReadonlyArray<[string, number]> = [
  ['s', 1],
  ['ms', 1e-3],
  ['μs', 1e-6],
  ['ns', 1e-9],
];

function axisScale(largest: number, unit?: string): AxisScale {
  const normalized = unit?.trim().toLowerCase() ?? '';
  const seconds = DURATION_SECONDS[normalized];
  if (seconds !== undefined) {
    const inSeconds = largest * seconds;
    const [label, size] = DURATION_STEPS.find(([, step]) => inSeconds >= step) ?? DURATION_STEPS[DURATION_STEPS.length - 1]!;
    return { factor: seconds / size, label: ` ${label}` };
  }
  if (['bytes', 'byte', 'decbytes', 'binb'].includes(normalized)) {
    const units = ['B', 'KiB', 'MiB', 'GiB', 'TiB', 'PiB'];
    let index = 0;
    while (largest / 1024 ** index >= 1024 && index < units.length - 1) index += 1;
    return { factor: 1 / 1024 ** index, label: ` ${units[index]}` };
  }
  if (['percent', 'percentunit', '%'].includes(normalized)) {
    return { factor: normalized === 'percentunit' ? 100 : 1, label: '%' };
  }
  const magnitudes: ReadonlyArray<[string, number]> = [
    ['T', 1e12],
    ['G', 1e9],
    ['M', 1e6],
    ['K', 1e3],
  ];
  const [suffix, size] = magnitudes.find(([, step]) => largest >= step) ?? ['', 1];
  const named = unit && !['short', 'none'].includes(normalized) ? ` ${unit}` : '';
  return { factor: 1 / size, label: `${suffix}${named}` };
}

/** The decimals the ticks need to stay apart: as many as the gap between neighbours asks, at most three. */
function axisStep(scaled: readonly number[]): number {
  const sorted = [...new Set(scaled)].sort((a, b) => a - b);
  let gap = Infinity;
  for (let index = 1; index < sorted.length; index += 1) gap = Math.min(gap, sorted[index]! - sorted[index - 1]!);
  if (!Number.isFinite(gap) || gap <= 0) {
    const largest = Math.max(...scaled.map((value) => Math.abs(value)));
    return largest >= 100 ? 0 : largest >= 10 ? 1 : 2;
  }
  return Math.min(3, Math.max(0, Math.ceil(-Math.log10(gap) + 1e-9)));
}

export function formatTimeSeriesAxisTimestamp(
  epochSeconds: number,
  visibleSpanSeconds: number,
  timezone = activeTimeZone(),
  /** The time between two ticks: seconds are written only when ticks fall within one minute. */
  tickStepSeconds?: number,
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
  if (tickStepSeconds !== undefined ? tickStepSeconds < 60 : visibleSpanSeconds <= 3600) {
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

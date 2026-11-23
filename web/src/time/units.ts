import { intlLocale } from '@/i18n/locales';

export type TimeUnit = 'millisecond' | 'second' | 'minute' | 'hour' | 'day';

/**
 * An amount of time as the reader's language writes it: "5 min", "5 Min.",
 * "5 分鐘". Fractions keep one decimal.
 */
export function formatTimeAmount(
  value: number,
  unit: TimeUnit,
  language: string,
  unitDisplay: 'long' | 'short' | 'narrow' = 'short',
): string {
  return new Intl.NumberFormat(intlLocale(language), {
    style: 'unit',
    unit,
    unitDisplay,
    maximumFractionDigits: 1,
  }).format(value);
}

/** The time unit a one-letter suffix such as the `m` of `5m` stands for. */
export const SUFFIX_UNITS: Record<string, TimeUnit> = {
  ms: 'millisecond',
  s: 'second',
  m: 'minute',
  h: 'hour',
  d: 'day',
};

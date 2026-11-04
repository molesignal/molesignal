const SECOND = 1_000_000;
const MINUTE = 60 * SECOND;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

type TimeUnit = 'second' | 'minute' | 'hour' | 'day';

function unitText(amount: number, unit: TimeUnit, language: string): string {
  return new Intl.NumberFormat(language, { style: 'unit', unit, unitDisplay: 'long', maximumFractionDigits: 1 }).format(amount);
}

/**
 * How long a range is, in the unit a reader counts it in: 24 hours, 90 minutes, 7 days. A day
 * is told as hours until it is two, as an hour is as minutes.
 */
export function spanText(micros: number, language: string): string {
  const [unit, size] =
    micros >= 2 * DAY
      ? (['day', DAY] as const)
      : micros >= 2 * HOUR
        ? (['hour', HOUR] as const)
        : micros >= 2 * MINUTE
          ? (['minute', MINUTE] as const)
          : (['second', SECOND] as const);
  const amount = micros / size;

  return unitText(amount >= 10 ? Math.round(amount) : Math.round(amount * 10) / 10, unit, language);
}

/** What a bar stands for, in the largest unit it is a whole number of: 1 hour, 5 minutes, 30 seconds. */
export function bucketText(micros: number, language: string): string {
  if (micros % DAY === 0) return unitText(micros / DAY, 'day', language);
  if (micros % HOUR === 0) return unitText(micros / HOUR, 'hour', language);
  if (micros % MINUTE === 0) return unitText(micros / MINUTE, 'minute', language);

  return unitText(micros / SECOND, 'second', language);
}

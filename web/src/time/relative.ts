const MICROS_PER_SECOND = 1_000_000;
const MILLIS_PER_MINUTE = 60_000;
const MILLIS_PER_HOUR = 60 * MILLIS_PER_MINUTE;
const MILLIS_PER_DAY = 24 * MILLIS_PER_HOUR;

export function formatRelativeMicros(
  micros: number | null | undefined,
  locale: string,
  nowMicros = Date.now() * 1000,
): string {
  if (!micros) return '—';

  const normalizedMicros =
    micros < 1e12 ? micros * MICROS_PER_SECOND : micros;
  const elapsedMillis = Math.max(0, (nowMicros - normalizedMicros) / 1000);
  const formatter = new Intl.RelativeTimeFormat(locale, { numeric: 'auto' });

  if (elapsedMillis < 45_000) return formatter.format(0, 'second');
  if (elapsedMillis < MILLIS_PER_HOUR) {
    return formatter.format(
      -Math.max(1, Math.round(elapsedMillis / MILLIS_PER_MINUTE)),
      'minute',
    );
  }
  if (elapsedMillis < MILLIS_PER_DAY) {
    return formatter.format(
      -Math.max(1, Math.round(elapsedMillis / MILLIS_PER_HOUR)),
      'hour',
    );
  }
  return formatter.format(
    -Math.max(1, Math.round(elapsedMillis / MILLIS_PER_DAY)),
    'day',
  );
}

/**
 * How far one moment is from another, in words: "38 minutes ago", "in 2 days".
 * Unlike `formatRelativeMicros` it looks both ways, and it is measured from a
 * moment the caller names (when a page was generated), so everyone reading the
 * same page reads the same words.
 */
export function formatRelativeSpan(targetMicros: number, fromMicros: number, locale: string): string {
  const seconds = Math.round(Math.abs(targetMicros - fromMicros) / MICROS_PER_SECOND);
  // The direction is read from the moments themselves: rounded to seconds, something that
  // began a moment ago would be at no distance at all, and would be said to lie ahead.
  const sign = targetMicros <= fromMicros ? -1 : 1;
  const formatter = new Intl.RelativeTimeFormat(locale, { numeric: 'always' });

  if (seconds < 90 * 60) return formatter.format(sign * Math.max(1, Math.round(seconds / 60)), 'minute');
  if (seconds < 36 * 3600) return formatter.format(sign * Math.round(seconds / 3600), 'hour');

  return formatter.format(sign * Math.round(seconds / 86_400), 'day');
}

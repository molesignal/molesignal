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

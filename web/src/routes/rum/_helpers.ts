import { formatMicrosActive } from '@/lib/time';
import { resolveWindow, useTimeStore, type TimeWindow } from '@/stores/useTimeStore';

export interface MicrosRange {
  from_micros: number;
  to_micros: number;
}

/** Convert the live time window into the µs range every RUM page needs. */
export function windowToMicros(
  window: TimeWindow = useTimeStore.getState().window,
  now: Date = new Date(),
): MicrosRange {
  const { from, to } = resolveWindow(window, now);
  return {
    from_micros: from.getTime() * 1000,
    to_micros: to.getTime() * 1000,
  };
}

/** 委托到中央层，按当前生效的全局时区 / 时间格式偏好渲染（ShellRoot 注入）。 */
export function formatMicros(micros: number | undefined): string {
  return formatMicrosActive(micros);
}

/**
 * A share as a percentage with the decimals it has, up to two: `75%` and
 * `99.97%`, not `75.00%`.
 */
export function formatShare(fraction: number): string {
  return `${Number((fraction * 100).toFixed(2))}%`;
}

/**
 * The name of a region in the language of the product, for a two-letter code:
 * `BR` reads "Brazil" or "巴西". Anything else is shown as it came.
 */
export function regionName(code: string, language: string | undefined): string {
  if (!/^[A-Za-z]{2}$/.test(code)) return code;

  try {
    return new Intl.DisplayNames([language ?? 'en'], { type: 'region' }).of(code.toUpperCase()) ?? code;
  } catch {
    return code;
  }
}

export function formatDurationMs(ms: number | undefined): string {
  if (ms == null) return '—';
  if (ms < 1000) return `${Math.round(ms)} ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)} s`;
  return `${(ms / 60_000).toFixed(1)} min`;
}

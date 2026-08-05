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

export function formatDurationMs(ms: number | undefined): string {
  if (ms == null) return '—';
  if (ms < 1000) return `${Math.round(ms)} ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)} s`;
  return `${(ms / 60_000).toFixed(1)} min`;
}

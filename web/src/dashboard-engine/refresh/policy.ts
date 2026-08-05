import type {
  DashboardRefreshSettings,
  DashboardTimeRange,
} from '../schema';

export type DashboardRefreshCadence = number | 'auto' | false;
export type DashboardTimeRangeResolver = (
  now?: Date,
) => DashboardTimeRange;

const AUTO_REFRESH_INTERVALS = [
  5_000,
  10_000,
  15_000,
  30_000,
  60_000,
  120_000,
  300_000,
  600_000,
  900_000,
  1_800_000,
  3_600_000,
] as const;

const FALLBACK_PANEL_WIDTH = 1_200;

export function refreshCadenceFromSettings(
  settings: DashboardRefreshSettings,
): DashboardRefreshCadence {
  if (!settings.enabled || settings.mode === 'off') return false;
  if (settings.mode === 'live') return 'auto';
  return parseIntervalMilliseconds(settings.defaultInterval);
}

export function resolveRefreshIntervalMilliseconds(
  cadence: DashboardRefreshCadence,
  timeRange: DashboardTimeRange,
  panelWidth: number,
): number | false {
  if (cadence === false) return false;
  if (cadence !== 'auto') return cadence;
  return autoRefreshIntervalMilliseconds(
    Math.max(0, timeRange.to - timeRange.from) / 1_000,
    panelWidth,
  );
}

/**
 * Advance roughly one horizontal pixel per refresh. The result is snapped to
 * a small set of human-readable intervals and never falls back to a 1s poll.
 */
export function autoRefreshIntervalMilliseconds(
  rangeMilliseconds: number,
  panelWidth: number,
): number {
  const effectiveWidth = Math.min(
    2_560,
    Math.max(320, Number.isFinite(panelWidth) ? panelWidth : FALLBACK_PANEL_WIDTH),
  );
  const target = Math.max(
    AUTO_REFRESH_INTERVALS[0],
    rangeMilliseconds / effectiveWidth,
  );
  return (
    AUTO_REFRESH_INTERVALS.find((interval) => interval >= target) ??
    3_600_000
  );
}

export function parseIntervalMilliseconds(value?: string): number | false {
  if (!value || value === 'off') return false;
  const match = /^(\d+)(ms|s|m|h|d)$/.exec(value.trim());
  if (!match) return false;
  const count = Number(match[1]);
  const unit = match[2];
  return unit === 'ms'
    ? count
    : unit === 's'
      ? count * 1_000
      : unit === 'm'
        ? count * 60_000
        : unit === 'h'
          ? count * 3_600_000
          : count * 86_400_000;
}

export function parseIntervalMicroseconds(value?: string): number | false {
  const milliseconds = parseIntervalMilliseconds(value);
  return milliseconds === false ? false : milliseconds * 1_000;
}

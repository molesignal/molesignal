import { useSearchParams } from 'react-router-dom';

import { resolveWindow, type TimeWindow } from '@/stores/useTimeStore';
import { useOpeningTimeWindow } from '@/time/openingWindow';

/** Checks run on schedules of minutes: a tab that names no range looks back a day. */
export const SYNTHETICS_WINDOW: TimeWindow = { from: 'now-24h', to: 'now', mode: 'relative' };

/** The range the timed tabs of Synthetics read. */
export function useSyntheticsTimeWindow(): TimeWindow {
  const [searchParams] = useSearchParams();
  return useOpeningTimeWindow(searchParams, SYNTHETICS_WINDOW).effectiveTimeWindow;
}

/** A range as the microseconds a request asks for, resolved at `now`. */
export function windowMicros(window: TimeWindow, now: Date = new Date()): { from: number; to: number } {
  const { from, to } = resolveWindow(window, now);
  return { from: from.getTime() * 1000, to: to.getTime() * 1000 };
}

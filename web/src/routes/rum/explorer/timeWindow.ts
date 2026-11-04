import type { TimeWindow } from '@/stores/useTimeStore';
import { useOpeningTimeWindow } from '@/time/openingWindow';

const RUM_DEFAULT_WINDOW: TimeWindow = {
  from: 'now-24h',
  to: 'now',
  mode: 'relative',
};

/** What a page that asks something over many days (funnels, paths, who comes back) looks back by default. */
export const RUM_WEEK_WINDOW: TimeWindow = {
  from: 'now-7d',
  to: 'now',
  mode: 'relative',
};

/**
 * The time range the RUM explorer queries.
 *
 * Real users come and go in the course of a day, so a visit that names no range looks back a day
 * instead of the app's default hour. That only applies when the page opens on the default: a
 * range the address names, or one chosen elsewhere, is kept. The store is the one in scope, so in
 * a query tab this is the tab's own range.
 *
 * A page can open on a longer range than the day, as long as it is always the same one.
 */
export function useRumTimeWindow(searchParams: URLSearchParams, opensOn: TimeWindow = RUM_DEFAULT_WINDOW) {
  return useOpeningTimeWindow(searchParams, opensOn);
}

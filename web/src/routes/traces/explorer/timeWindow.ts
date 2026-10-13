import * as React from 'react';

import { isDefaultTimeWindow, type TimeWindow, useTimeStore } from '@/stores/useTimeStore';

const TRACE_DEFAULT_WINDOW: TimeWindow = {
  from: 'now-24h',
  to: 'now',
  mode: 'relative',
};

/**
 * The time range the trace explorer queries.
 *
 * Traces are sparser than logs, so a visit that names no range looks back a day
 * instead of the app's default hour. That only applies when the page opens on
 * the default: a range the address names, or one chosen elsewhere, is kept.
 * The store is the one in scope, so in a query tab this is the tab's own range.
 */
export function useTraceTimeWindow(searchParams: URLSearchParams) {
  const timeWindow = useTimeStore((state) => state.window);
  const setTimeWindow = useTimeStore((state) => state.setWindow);
  const applyDefault = React.useRef(
    !searchParams.has('time')
      && !(searchParams.has('from') && searchParams.has('to'))
      && isDefaultTimeWindow(timeWindow),
  );
  const shouldApplyDefault = applyDefault.current;

  React.useLayoutEffect(() => {
    applyDefault.current = false;
    if (shouldApplyDefault) setTimeWindow(TRACE_DEFAULT_WINDOW);
  }, [setTimeWindow, shouldApplyDefault]);

  return {
    timeWindow,
    /** The range to query now: the day-long default until the store has taken it. */
    effectiveTimeWindow: shouldApplyDefault ? TRACE_DEFAULT_WINDOW : timeWindow,
    setTimeWindow,
  };
}

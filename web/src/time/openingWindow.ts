import * as React from 'react';

import { isDefaultTimeWindow, type TimeWindow, useTimeStore } from '@/stores/useTimeStore';

/**
 * The time range a page of a module queries, when the module looks back further than the app's
 * default hour.
 *
 * The longer range only applies when the page opens on the default: a range the address names,
 * or one chosen elsewhere, is kept. The store is the one in scope, so in a query tab this is the
 * tab's own range.
 */
export function useOpeningTimeWindow(searchParams: URLSearchParams, opensOn: TimeWindow) {
  const timeWindow = useTimeStore((state) => state.window);
  const setTimeWindow = useTimeStore((state) => state.setWindow);
  const applyDefault = React.useRef(
    !searchParams.has('time') &&
      !(searchParams.has('from') && searchParams.has('to')) &&
      isDefaultTimeWindow(timeWindow),
  );
  const shouldApplyDefault = applyDefault.current;

  React.useLayoutEffect(() => {
    applyDefault.current = false;
    if (shouldApplyDefault) setTimeWindow(opensOn);
  }, [opensOn, setTimeWindow, shouldApplyDefault]);

  return {
    timeWindow,
    /** The range to query now: the module's own until the store has taken it. */
    effectiveTimeWindow: shouldApplyDefault ? opensOn : timeWindow,
    setTimeWindow,
  };
}

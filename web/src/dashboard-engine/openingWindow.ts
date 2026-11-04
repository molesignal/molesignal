import * as React from 'react';

import { isDefaultTimeWindow, useTimeStore } from '@/stores/useTimeStore';

import type { DashboardTimeSettings } from './schema';

/**
 * Whether a stored model is one of the dashboards the product installs for an organization
 * (`extensions.builtin`).
 */
export function isBuiltinModel(model: Record<string, unknown> | undefined): boolean {
  const extensions = model?.extensions;

  return typeof extensions === 'object' && extensions !== null && 'builtin' in extensions;
}

/**
 * A dashboard the product installs opens on the range it was made for (a day, for the RUM ones)
 * rather than on the hour the app starts with. That is only so when nothing else chose a range: one
 * the address names, or one chosen elsewhere, is kept. Other dashboards keep the range in view.
 */
export function useOpeningWindow(
  uid: string | undefined,
  settings: DashboardTimeSettings | undefined,
  searchParams: URLSearchParams,
): void {
  const timeWindow = useTimeStore((state) => state.window);
  const setTimeWindow = useTimeStore((state) => state.setWindow);
  const openedOn = React.useRef<string | null>(null);

  React.useLayoutEffect(() => {
    if (!uid || !settings || openedOn.current === uid) return;

    openedOn.current = uid;

    const named = searchParams.has('time') || (searchParams.has('from') && searchParams.has('to'));

    if (named || !isDefaultTimeWindow(timeWindow)) return;

    const relative = settings.defaultFrom.startsWith('now');

    setTimeWindow({
      from: settings.defaultFrom,
      to: settings.defaultTo,
      mode: relative ? 'relative' : 'absolute',
    });
  }, [uid, settings, searchParams, timeWindow, setTimeWindow]);
}

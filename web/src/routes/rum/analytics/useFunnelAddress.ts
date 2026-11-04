import * as React from 'react';
import { useSearchParams } from 'react-router-dom';

import { decodeSteps, emptyStep, encodeSteps, MIN_STEPS, type StepDraft } from './steps';

/** The windows a funnel can be limited to, in seconds; none lets a session take as long as it takes. */
export const WINDOWS = [300, 1_800, 3_600, 86_400] as const;

/** The fields sessions can be counted by in a funnel, besides all of them together. */
export const BREAKDOWNS = ['version', 'browser.name', 'os.name', 'device.type', 'geo.country_iso_code'] as const;

/**
 * The funnel in the address: the steps, the window and the field the sessions are counted by, so a
 * funnel is a link. A funnel with fewer than two steps is given empty ones to fill in.
 */
export function useFunnelAddress(browser: boolean) {
  const [params, setParams] = useSearchParams();
  const text = params.get('steps');

  const steps = React.useMemo(() => {
    const parsed = decodeSteps(text);
    const missing = Math.max(0, MIN_STEPS - parsed.length);

    return [...parsed, ...Array.from({ length: missing }, () => emptyStep('views', browser))];
  }, [browser, text]);

  const windowSecs = Number(params.get('win')) || null;
  const rawBreakdown = params.get('by') ?? '';
  const breakdown = (BREAKDOWNS as readonly string[]).includes(rawBreakdown) ? rawBreakdown : '';

  const change = React.useCallback(
    (name: string, value: string | null) =>
      setParams(
        (current) => {
          const next = new URLSearchParams(current);

          if (value) next.set(name, value);
          else next.delete(name);

          return next;
        },
        { replace: true },
      ),
    [setParams],
  );

  return {
    steps,
    windowSecs,
    breakdown,
    setSteps: (next: readonly StepDraft[]) => change('steps', encodeSteps(next)),
    setWindow: (seconds: number | null) => change('win', seconds ? String(seconds) : null),
    setBreakdown: (field: string) => change('by', field || null),
  };
}

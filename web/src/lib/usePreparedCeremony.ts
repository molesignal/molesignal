import * as React from 'react';

import type { PasskeyCeremony } from '@/lib/webauthn';

/** A ceremony lives five minutes on the server; a fresh one is fetched before that. */
const REFRESH_MS = 4 * 60_000;

/**
 * Fetches a passkey ceremony ahead of the button that uses it: Safari only lets a
 * click start the browser's prompt, and a request in between would use the click up.
 * `attempt` going up fetches a new one, for after the server used the last one up.
 */
export function usePreparedCeremony<Options>(
  load: () => Promise<PasskeyCeremony<Options>>,
  active: boolean,
  attempt: number,
  onError: (error: unknown) => void,
): PasskeyCeremony<Options> | null {
  const [prepared, setPrepared] = React.useState<PasskeyCeremony<Options> | null>(null);
  const [refresh, setRefresh] = React.useState(0);
  const latest = React.useRef({ load, onError });
  latest.current = { load, onError };

  React.useEffect(() => {
    setPrepared(null);
    if (!active) return undefined;

    let stale = false;
    const timer = window.setTimeout(() => setRefresh((count) => count + 1), REFRESH_MS);

    latest.current
      .load()
      .then((next) => {
        if (!stale) setPrepared(next);
      })
      .catch((error: unknown) => {
        if (!stale) latest.current.onError(error);
      });

    return () => {
      stale = true;
      window.clearTimeout(timer);
    };
  }, [active, attempt, refresh]);

  return prepared;
}

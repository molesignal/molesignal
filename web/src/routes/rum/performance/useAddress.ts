import * as React from 'react';
import { useSearchParams } from 'react-router-dom';

/** What a view changes in its address: a parameter set to a value, or taken out with none. */
export type Changes = Record<string, string | null | undefined>;

/**
 * The state of a view lives in its address, so a view can be sent as it stands. A change of what is
 * shown replaces the entry; opening something adds one, so Back closes it. The view says how it reads
 * its parameters and how a change writes them.
 */
export function useAddressState<T>(
  read: (params: URLSearchParams) => T,
  apply: (current: URLSearchParams, changes: Changes) => URLSearchParams,
) {
  const [params, setParams] = useSearchParams();
  const view = React.useMemo(() => read(params), [params, read]);

  const set = React.useCallback(
    (changes: Changes, options: { push?: boolean } = {}) =>
      setParams((current) => apply(current, changes), { replace: !options.push }),
    [apply, setParams],
  );

  return { view, set };
}

/** The address after changes: each parameter set to its value, or taken out when it has none. */
export function written(current: URLSearchParams, changes: Changes): URLSearchParams {
  const next = new URLSearchParams(current);

  for (const [name, value] of Object.entries(changes)) {
    if (value) next.set(name, value);
    else next.delete(name);
  }

  return next;
}

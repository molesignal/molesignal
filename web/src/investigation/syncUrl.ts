import * as React from 'react';
import { useLocation, useNavigate } from 'react-router-dom';

import { encodeFilters, encodeStack } from '@/shell/UrlHydration';
import { useFiltersStore } from '@/stores/useFiltersStore';
import { useInvestigationStack } from '@/stores/useInvestigationStack';
import { useTimeStore, writeTimeParams } from '@/stores/useTimeStore';

/**
 * Subscribes to investigation-stack + time stores and mirrors their state into
 * the URL search params. The reverse direction (URL → store) is handled by
 * `hydrateFromSearchParams` in `shell/UrlHydration.ts`, called from ShellRoot.
 *
 * Mount this hook once at the route shell layer. It is a no-op until at least
 * one frame or non-default time anchor is set.
 */
export function useSyncStateToUrl() {
  const location = useLocation();
  const navigate = useNavigate();
  const frames = useInvestigationStack((s) => s.frames);
  const filters = useFiltersStore((s) => s.filters);
  const window = useTimeStore((s) => s.window);
  const anchor = useTimeStore((s) => s.anchor);

  // Debounce writes; rapid keyboard navigation otherwise spams history.
  React.useEffect(() => {
    const handle = globalThis.setTimeout(() => {
      // Router rendering may lag behind navigation under startTransition. Merge
      // into the address that is actually current, never a captured query string.
      const address = globalThis.location;
      if (address.pathname !== location.pathname) return;
      const next = new URLSearchParams(address.search);
      const time = useTimeStore.getState();
      writeTimeParams(next, time.window, time.anchor);
      // stack
      if (frames.length === 0) {
        next.delete('stack');
      } else {
        next.set('stack', encodeStack(frames));
      }
      // pinned filters
      if (filters.length === 0) next.delete('filters');
      else next.set('filters', encodeFilters(filters));
      const search = next.size > 0 ? `?${next}` : '';
      if (search === address.search) return;
      navigate(
        {
          pathname: address.pathname,
          search,
          hash: address.hash,
        },
        { replace: true },
      );
    }, 80);
    return () => globalThis.clearTimeout(handle);
  }, [anchor, filters, frames, location.hash, location.pathname, location.search, navigate, window]);
}

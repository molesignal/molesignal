import * as React from 'react';
import { useLocation, useNavigate } from 'react-router-dom';

import { encodeStack } from '@/shell/UrlHydration';
import { useInvestigationStack } from '@/stores/useInvestigationStack';
import { useTimeStore } from '@/stores/useTimeStore';

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
  const window = useTimeStore((s) => s.window);
  const anchor = useTimeStore((s) => s.anchor);

  // Debounce writes; rapid keyboard navigation otherwise spams history.
  React.useEffect(() => {
    const handle = globalThis.setTimeout(() => {
      const next = new URLSearchParams(location.search);
      // time
      if (window.from === 'now-1h' && window.to === 'now' && window.mode === 'relative') {
        next.delete('time');
      } else {
        next.set('time', `${window.from}..${window.to}`);
      }
      // anchor
      if (anchor) next.set('anchor', anchor.at);
      else next.delete('anchor');
      // stack
      if (frames.length === 0) {
        next.delete('stack');
      } else {
        next.set('stack', encodeStack(frames));
      }
      const search = next.size > 0 ? `?${next}` : '';
      if (search === location.search) return;
      navigate(
        {
          pathname: location.pathname,
          search,
          hash: location.hash,
        },
        { replace: true },
      );
    }, 80);
    return () => globalThis.clearTimeout(handle);
  }, [anchor, frames, location.hash, location.pathname, location.search, navigate, window]);
}

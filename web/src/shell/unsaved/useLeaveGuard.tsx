import * as React from 'react';
import { type BlockerFunction, useBlocker } from 'react-router-dom';

import { UnsavedPrompt } from './UnsavedPrompt';

/**
 * Keeps a page's unsaved work from being lost by leaving it: another page asks
 * first, a closed or reloaded tab gets the browser's own question. Only a change
 * of path counts; the page's own search parameters (the time range, a tab) do
 * not leave it.
 *
 * Draw `prompt` once in the page. Call `allowLeave()` right before a navigation
 * that follows a save: the save's state has not reached the guard at that moment.
 */
export function useLeaveGuard(dirty: boolean): { prompt: React.ReactNode; allowLeave: () => void } {
  const allowed = React.useRef(false);

  const blocker = useBlocker(
    React.useCallback<BlockerFunction>(
      ({ currentLocation, nextLocation }) =>
        dirty && !allowed.current && currentLocation.pathname !== nextLocation.pathname,
      [dirty],
    ),
  );

  React.useEffect(() => {
    if (!dirty) return;

    const warn = (event: BeforeUnloadEvent) => {
      if (allowed.current) return;
      event.preventDefault();
    };

    window.addEventListener('beforeunload', warn);
    return () => window.removeEventListener('beforeunload', warn);
  }, [dirty]);

  return {
    allowLeave: () => {
      allowed.current = true;
    },
    prompt: (
      <UnsavedPrompt
        open={blocker.state === 'blocked'}
        kind="leave"
        onKeep={() => blocker.reset?.()}
        onDiscard={() => blocker.proceed?.()}
      />
    ),
  };
}

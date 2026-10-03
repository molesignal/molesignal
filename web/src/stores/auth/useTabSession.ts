import * as React from 'react';

import { useOrgStore } from '@/stores/useOrgStore';

import { useAuthStore } from './index';
import { type SharedSession, tabSessionSharing } from './tabSession';

// Only the first authenticated screen of a page load may borrow a session. A
// tab that signs out later must not ask again: a tab that is slow to hear about
// the sign-out could hand the old session straight back.
let firstGateSeen = false;
let borrowing: Promise<void> | null = null;

function adopt(session: SharedSession | null): void {
  if (!session) return;
  const { token, setSession } = useAuthStore.getState();
  if (token) return;
  setSession(session.token, session.ctx);
}

/** Another tab signed out on purpose: leave nothing of the session behind here. */
function signOutHere(): void {
  useAuthStore.getState().logout();
  useOrgStore.getState().reset();
  // A fresh page rather than a client-side redirect, so the query cache and the
  // other in-memory state of the old session go too.
  const next = encodeURIComponent(window.location.pathname + window.location.search);
  window.location.assign(`/signin?next=${next}`);
}

/**
 * Shares the tab's session with the other open tabs (see `tabSession.ts`).
 * Returns `true` while a tab that was just opened without a session is still
 * asking the others for theirs, so the caller can hold off on the sign-in page.
 */
export function useTabSession(signedIn: boolean): boolean {
  const [startedSignedIn] = React.useState(signedIn);
  const [asking, setAsking] = React.useState(() => !signedIn && !firstGateSeen);

  React.useEffect(() => {
    if (!startedSignedIn && !firstGateSeen) {
      borrowing = tabSessionSharing.borrowSession().then(adopt);
    }
    firstGateSeen = true;
    if (!borrowing) return;

    let mounted = true;
    void borrowing.then(() => {
      if (mounted) setAsking(false);
    });
    return () => {
      mounted = false;
    };
  }, [startedSignedIn]);

  React.useEffect(() => {
    if (!signedIn) return;
    return tabSessionSharing.lendSession({
      getSession: () => {
        const { token, ctx } = useAuthStore.getState();
        return token && ctx ? { token, ctx } : null;
      },
      onSignOut: signOutHere,
    });
  }, [signedIn]);

  return asking;
}

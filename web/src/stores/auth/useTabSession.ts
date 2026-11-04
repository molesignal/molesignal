import * as React from 'react';

import { adoptCurrentSession, currentSession } from '@/lib/http';
import { useOrgStore } from '@/stores/useOrgStore';

import { sessionKey, useAuthStore } from './index';
import { tabSessionSharing } from './tabSession';

// Only the first authenticated screen of a page load asks the server for the
// session. A tab that signs out later goes to the sign-in page without asking.
let firstGateSeen = false;
let asking: Promise<void> | null = null;

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
 * Keeps the tab's session in step with the browser's (see `tabSession.ts`).
 * A tab opened by hand has no session of its own yet, though the browser's
 * session cookie may hold one: it asks the server, and this returns `true`
 * while that question is open so the caller can hold off on the sign-in page.
 */
export function useTabSession(signedIn: boolean): boolean {
  const [startedSignedIn] = React.useState(signedIn);
  const [checking, setChecking] = React.useState(() => !signedIn && !firstGateSeen);

  React.useEffect(() => {
    if (!startedSignedIn && !firstGateSeen) {
      asking = currentSession().then((session) => {
        if (session && !useAuthStore.getState().ctx) useAuthStore.getState().setSession(session);
      });
    }
    firstGateSeen = true;
    if (!asking) return;

    let mounted = true;
    void asking.then(() => {
      if (mounted) setChecking(false);
    });
    return () => {
      mounted = false;
    };
  }, [startedSignedIn]);

  React.useEffect(() => {
    if (!signedIn) return;
    return tabSessionSharing.listen({
      onSignOut: signOutHere,
      onSession: (session) => {
        const ctx = useAuthStore.getState().ctx;
        if (ctx && sessionKey(ctx) !== session) void adoptCurrentSession();
      },
    });
  }, [signedIn]);

  return checking;
}

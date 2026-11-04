import * as React from 'react';
import { Navigate, useLocation } from 'react-router-dom';

import { useAuthStore } from '@/stores/auth';
import { useTabSession } from '@/stores/auth/useTabSession';

export function RequireAuth({ children }: { children: React.ReactNode }) {
  const ctx = useAuthStore((s) => s.ctx);
  const location = useLocation();
  const signedIn = Boolean(ctx);
  const checking = useTabSession(signedIn);
  if (!signedIn) {
    // A tab opened by hand has no session of its own: give the server a moment
    // to say whether the browser is signed in before asking the user to sign in.
    if (checking) return null;
    const next = encodeURIComponent(location.pathname + location.search);
    return <Navigate to={`/signin?next=${next}`} replace />;
  }
  return <>{children}</>;
}

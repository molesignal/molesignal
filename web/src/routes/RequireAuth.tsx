import * as React from 'react';
import { Navigate, useLocation } from 'react-router-dom';

import { useAuthStore } from '@/stores/auth';
import { useTabSession } from '@/stores/auth/useTabSession';

export function RequireAuth({ children }: { children: React.ReactNode }) {
  const token = useAuthStore((s) => s.token);
  const ctx = useAuthStore((s) => s.ctx);
  const location = useLocation();
  const signedIn = Boolean(token && ctx);
  const askingOtherTabs = useTabSession(signedIn);
  if (!signedIn) {
    // A tab opened by hand has no session of its own: give the tabs that are
    // already open a moment to hand theirs over before asking the user to sign in.
    if (askingOtherTabs) return null;
    const next = encodeURIComponent(location.pathname + location.search);
    return <Navigate to={`/signin?next=${next}`} replace />;
  }
  return <>{children}</>;
}

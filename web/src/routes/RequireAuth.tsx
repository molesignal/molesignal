import * as React from 'react';
import { Navigate, useLocation } from 'react-router-dom';

import { useAuthStore } from '@/stores/auth';

export function RequireAuth({ children }: { children: React.ReactNode }) {
  const token = useAuthStore((s) => s.token);
  const ctx = useAuthStore((s) => s.ctx);
  const location = useLocation();
  if (!token || !ctx) {
    const next = encodeURIComponent(location.pathname + location.search);
    return <Navigate to={`/signin?next=${next}`} replace />;
  }
  return <>{children}</>;
}

import * as React from 'react';
import { useLocation, useNavigate, useSearchParams } from 'react-router-dom';

import { useActionAccess } from '@/product/actionAccess';

import { AlertIncidentsPage } from './incidents';
import { AlertRulesPage } from './rules';

/** The two tabs that share the Alerts routes: what is firing, and the rules that decide it. */
export function Alerts() {
  const location = useLocation();
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const createAccess = useActionAccess({ permission: 'alerts.manage' });

  React.useEffect(() => {
    if (searchParams.get('create') === '1' && createAccess.allowed) {
      navigate('/alerts/rules/new', { replace: true });
    }
  }, [createAccess.allowed, navigate, searchParams]);

  if (location.pathname.startsWith('/alerts/rules')) return <AlertRulesPage />;

  return <AlertIncidentsPage />;
}

import {
  Activity,
  Boxes,
  Bug,
  Gauge,
  GitBranch,
  Network,
  Waypoints,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { NavLink, Outlet, useLocation } from 'react-router-dom';

import { cn } from '@/shell/lib/cn';

const NAV = [
  { to: '/apm/overview', key: 'overview', icon: Gauge },
  { to: '/apm/services', key: 'services', icon: Activity },
  { to: '/apm/transactions', key: 'transactions', icon: Boxes },
  { to: '/traces', key: 'traces', icon: Waypoints },
  { to: '/apm/dependencies', key: 'dependencies', icon: Network },
  { to: '/apm/errors', key: 'errors', icon: Bug },
  { to: '/apm/deployments', key: 'deployments', icon: GitBranch },
] as const;

export function ApmLayout() {
  return <Outlet />;
}

export function ApmNavigation() {
  const { t } = useTranslation('apm');
  const location = useLocation();
  return (
    <nav
      aria-label={t('title')}
      className="flex min-h-11 items-stretch gap-1 overflow-x-auto border-b border-bd-0 bg-bg-1 px-6"
    >
      {NAV.map(({ to, key, icon: Icon }) => (
        <NavLink
          key={to}
          to={{ pathname: to, search: location.search }}
          className={({ isActive }) =>
            cn(
              'inline-flex shrink-0 items-center gap-2 border-b-2 border-transparent px-3 text-xs font-strong text-tx-2 transition-colors',
              'outline-none hover:bg-bg-2 hover:text-tx-0 focus-visible:bg-bg-2 focus-visible:text-tx-0',
              isActive && 'border-indigo bg-bg-2 text-tx-0',
            )
          }
        >
          <Icon aria-hidden className="h-3.5 w-3.5" />
          {t(`nav.${key}`)}
        </NavLink>
      ))}
    </nav>
  );
}

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
import {
  surfaceModuleNavigationActiveClass,
  surfaceModuleNavigationClass,
  surfaceModuleNavigationItemClass,
  surfaceModuleNavigationRowClass,
} from '@/shell/SurfaceWorkbench';

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
    <div
      data-apm-navigation="surface"
      className={surfaceModuleNavigationClass}
    >
      <nav aria-label={t('title')} className={surfaceModuleNavigationRowClass}>
        {NAV.map(({ to, key, icon: Icon }) => (
          <NavLink
            key={to}
            to={{ pathname: to, search: location.search }}
            className={({ isActive }) =>
              cn(
                surfaceModuleNavigationItemClass,
                'gap-2',
                isActive && surfaceModuleNavigationActiveClass,
              )
            }
          >
            <Icon aria-hidden className="h-3.5 w-3.5" />
            {t(`nav.${key}`)}
          </NavLink>
        ))}
      </nav>
    </div>
  );
}

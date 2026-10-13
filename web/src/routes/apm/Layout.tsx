import {
  Activity,
  Boxes,
  Bug,
  Gauge,
  GitBranch,
  Network,
  Waypoints,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Outlet, useLocation } from 'react-router-dom';

import { ModuleTabs } from '@/shell/tabs/ModuleTabs';

import { ApmNavigationSlots } from './navigationSlots';

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
  const { end, scope } = React.useContext(ApmNavigationSlots);
  return (
    <ModuleTabs
      label={t('title')}
      dataAttributes={{ 'data-apm-navigation': 'surface' }}
      end={end}
      items={NAV.map(({ to, key, icon }) => ({
        key,
        // The filters in the query string follow the user from tab to tab.
        to: { pathname: to, search: location.search },
        icon,
        label: t(`nav.${key}`),
      }))}
    >
      <ApmScopeRow>{scope}</ApmScopeRow>
    </ModuleTabs>
  );
}

/** What the page is scoped to, as the second row of the navigation card. */
export function ApmScopeRow({ children }: { children?: React.ReactNode }) {
  if (!children) return null;
  return (
    // Starts where the first tab's icon does, so the two rows share a left edge.
    <div data-apm-scope className="border-t border-bd-0 px-[20px] py-[8px]">
      {children}
    </div>
  );
}

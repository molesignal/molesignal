import { Boxes, Bug, Gauge, Network, Settings2, Target } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useLocation } from 'react-router-dom';

import { useOpenablePages } from '@/product/moduleAccess';
import { ModuleTabs } from '@/shell/tabs/ModuleTabs';

const NAV = [
  { to: '/apm/overview', key: 'overview', icon: Gauge },
  { to: '/apm/services', key: 'services', icon: Boxes },
  { to: '/apm/map', key: 'map', icon: Network },
  { to: '/apm/errors', key: 'errors', icon: Bug },
  { to: '/apm/slos', key: 'slos', icon: Target },
  { to: '/apm/settings', key: 'settings', icon: Settings2 },
] as const;

/** The module's pages, in the order it opens on the first the reader may see. */
export const APM_PAGES = NAV.map(({ to }) => to);

/** What follows the reader from one APM page to another: where and how the services are looked at. */
const CARRIED = ['environment', 'resolution', 'compare'] as const;

/**
 * The APM module's navigation card: all services together, each of them,
 * their map and their errors, with the time controls at the end of the row
 * and what the page is scoped to in a row under it.
 */
export function ApmNavigation({ end, children }: { end?: React.ReactNode; children?: React.ReactNode }) {
  const { t } = useTranslation('apm');
  const location = useLocation();
  const params = new URLSearchParams(location.search);
  const carried = new URLSearchParams();

  for (const key of CARRIED) {
    const value = params.get(key);
    if (value) carried.set(key, value);
  }

  const search = carried.size ? `?${carried}` : '';
  const pages = useOpenablePages(NAV, ({ to }) => to);

  return (
    <ModuleTabs
      label={t('title')}
      dataAttributes={{ 'data-apm-navigation': 'surface' }}
      end={end}
      items={pages.map(({ to, key, icon }) => ({
        key,
        to: { pathname: to, search },
        icon,
        label: t(`nav.${key}`),
      }))}
    >
      {children && <ApmScopeRow>{children}</ApmScopeRow>}
    </ModuleTabs>
  );
}

/** What the page is scoped to, as the second row of a navigation card. */
export function ApmScopeRow({ children }: { children: React.ReactNode }) {
  return (
    // Starts where the first tab's icon does, so the two rows share a left edge.
    <div data-apm-scope className="flex flex-wrap items-center gap-2 border-t border-bd-0 px-[20px] py-[8px]">
      {children}
    </div>
  );
}

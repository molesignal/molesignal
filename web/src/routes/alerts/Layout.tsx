import { useTranslation } from 'react-i18next';
import { NavLink } from 'react-router-dom';

import { cn } from '@/shell/lib/cn';
import {
  surfaceModuleNavigationActiveClass,
  surfaceModuleNavigationClass,
  surfaceModuleNavigationItemClass,
  surfaceModuleNavigationRowClass,
} from '@/shell/SurfaceWorkbench';

const ALERT_TABS = [
  { to: '/alerts/insights', labelKey: 'subnav.insights', fallback: 'Insights' },
  { to: '/alerts/incidents', labelKey: 'subnav.incidents', fallback: 'Incidents' },
  { to: '/alerts/rules', labelKey: 'subnav.rules', fallback: 'Rules' },
  { to: '/alerts/history', labelKey: 'subnav.history', fallback: 'History' },
  { to: '/alerts/silences', labelKey: 'subnav.silences', fallback: 'Silences' },
  { to: '/alerts/escalations', labelKey: 'subnav.escalations', fallback: 'Escalations' },
  { to: '/alerts/schedules', labelKey: 'subnav.schedules', fallback: 'On-call schedules' },
  { to: '/alerts/semantic-groups', labelKey: 'subnav.groups', fallback: 'Groups' },
] as const;

/**
 * All alert-center destinations share one persistent navigation row.
 * The row scrolls horizontally on narrow viewports instead of hiding
 * operational destinations behind a second navigation model.
 */
export function AlertsSubNav() {
  const { t } = useTranslation('alerts');

  return (
    <div
      data-testid="alerts-subnav"
      className={cn(surfaceModuleNavigationClass, 'relative z-10')}
    >
      <nav
        aria-label={t('subnav.label', { defaultValue: 'Alerts views' })}
        className={surfaceModuleNavigationRowClass}
      >
        {ALERT_TABS.map((tab) => (
          <NavLink
            key={tab.to}
            to={tab.to}
            className={({ isActive }) =>
              cn(
                surfaceModuleNavigationItemClass,
                'whitespace-nowrap',
                isActive
                  ? surfaceModuleNavigationActiveClass
                  : 'text-tx-2',
              )
            }
          >
            {t(tab.labelKey, { defaultValue: tab.fallback })}
          </NavLink>
        ))}
      </nav>
    </div>
  );
}

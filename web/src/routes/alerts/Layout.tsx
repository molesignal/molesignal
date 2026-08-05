import { useTranslation } from 'react-i18next';
import { NavLink } from 'react-router-dom';

import { cn } from '@/shell/lib/cn';

const ALERT_TABS = [
  { to: '/alerts/incidents', labelKey: 'subnav.incidents', fallback: 'Incidents' },
  { to: '/alerts/rules', labelKey: 'subnav.rules', fallback: 'Rules' },
  { to: '/alerts/history', labelKey: 'subnav.history', fallback: 'History' },
  { to: '/alerts/insights', labelKey: 'subnav.insights', fallback: 'Insights' },
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
    <nav
      data-testid="alerts-subnav"
      aria-label={t('subnav.label', { defaultValue: 'Alerts views' })}
      className="relative z-10 -mt-px flex h-11 min-w-0 items-center border-b border-bd-0 bg-bg-1 px-3"
    >
      <div className="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto overflow-y-hidden">
        {ALERT_TABS.map((tab) => (
          <NavLink
            key={tab.to}
            to={tab.to}
            className={({ isActive }) =>
              cn(
                'inline-flex h-9 shrink-0 items-center whitespace-nowrap rounded-md px-3 font-sans text-xs font-strong text-tx-2',
                'transition-colors duration-fast ease-default',
                'hover:bg-bg-2 hover:text-tx-0',
                isActive
                  ? 'bg-bg-2 text-tx-0'
                  : 'text-tx-2',
              )
            }
          >
            {t(tab.labelKey, { defaultValue: tab.fallback })}
          </NavLink>
        ))}
      </div>
    </nav>
  );
}

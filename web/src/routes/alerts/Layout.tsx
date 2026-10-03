import { useTranslation } from 'react-i18next';

import { ModuleTabs } from '@/shell/tabs/ModuleTabs';

// What is happening and what defines it, then who is told and how.
const ALERT_TABS = [
  { to: '/alerts/insights', labelKey: 'subnav.insights', fallback: 'Insights', group: 'overview' },
  { to: '/alerts/incidents', labelKey: 'subnav.incidents', fallback: 'Incidents', group: 'overview' },
  { to: '/alerts/rules', labelKey: 'subnav.rules', fallback: 'Rules', group: 'overview' },
  { to: '/alerts/history', labelKey: 'subnav.history', fallback: 'History', group: 'overview' },
  { to: '/alerts/silences', labelKey: 'subnav.silences', fallback: 'Silences', group: 'response' },
  { to: '/alerts/escalations', labelKey: 'subnav.escalations', fallback: 'Escalations', group: 'response' },
  { to: '/alerts/schedules', labelKey: 'subnav.schedules', fallback: 'On-call schedules', group: 'response' },
  { to: '/alerts/semantic-groups', labelKey: 'subnav.groups', fallback: 'Groups', group: 'response' },
] as const;

/**
 * All alert-center destinations share one persistent navigation row.
 * The row scrolls horizontally on narrow viewports instead of hiding
 * operational destinations behind a second navigation model.
 */
export function AlertsSubNav() {
  const { t } = useTranslation('alerts');

  return (
    <ModuleTabs
      label={t('subnav.label', { defaultValue: 'Alerts views' })}
      className="relative z-10"
      dataAttributes={{ 'data-testid': 'alerts-subnav' }}
      items={ALERT_TABS.map((tab) => ({
        key: tab.to,
        to: tab.to,
        group: tab.group,
        label: t(tab.labelKey, { defaultValue: tab.fallback }),
      }))}
    />
  );
}

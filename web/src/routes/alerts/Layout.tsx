import type * as React from 'react';
import { useTranslation } from 'react-i18next';

import { useOpenablePages } from '@/product/moduleAccess';
import { ModuleTabs } from '@/shell/tabs/ModuleTabs';

// What is happening and what defines it, then who is told and how.
const ALERT_TABS = [
  { to: '/alerts/insights', labelKey: 'subnav.insights', fallback: 'Insights', group: 'overview' },
  { to: '/alerts/discovery', labelKey: 'subnav.discovery', fallback: 'Discovery', group: 'overview' },
  { to: '/alerts/incidents', labelKey: 'subnav.incidents', fallback: 'Incidents', group: 'overview' },
  { to: '/alerts/rules', labelKey: 'subnav.rules', fallback: 'Rules', group: 'overview' },
  { to: '/alerts/sources', labelKey: 'subnav.sources', fallback: 'Sources', group: 'overview' },
  { to: '/alerts/history', labelKey: 'subnav.history', fallback: 'History', group: 'overview' },
  { to: '/alerts/postmortems', labelKey: 'subnav.postmortems', fallback: 'Postmortems', group: 'overview' },
  { to: '/alerts/silences', labelKey: 'subnav.silences', fallback: 'Silences', group: 'response' },
  { to: '/alerts/inhibitions', labelKey: 'subnav.inhibitions', fallback: 'Inhibition', group: 'response' },
  { to: '/alerts/routing', labelKey: 'subnav.routing', fallback: 'Routing', group: 'response' },
  { to: '/alerts/escalations', labelKey: 'subnav.escalations', fallback: 'Escalations', group: 'response' },
  { to: '/alerts/schedules', labelKey: 'subnav.schedules', fallback: 'On-call schedules', group: 'response' },
  { to: '/alerts/remediation', labelKey: 'subnav.remediation', fallback: 'Remediation', group: 'response' },
  { to: '/alerts/tickets', labelKey: 'subnav.tickets', fallback: 'Tickets', group: 'response' },
  { to: '/alerts/semantic-groups', labelKey: 'subnav.groups', fallback: 'Groups', group: 'response' },
] as const;

/** Where the module opens, for whoever may read the incidents, and failing that its other pages. */
export const ALERT_PAGES = [
  '/alerts/incidents',
  ...ALERT_TABS.map((tab) => tab.to),
] as const;

/**
 * All alert-center destinations share one persistent navigation row.
 * The row scrolls horizontally on narrow viewports instead of hiding
 * operational destinations behind a second navigation model.
 */
export function AlertsSubNav({ end }: { end?: React.ReactNode } = {}) {
  const { t } = useTranslation('alerts');
  const tabs = useOpenablePages(ALERT_TABS, (tab) => tab.to);

  return (
    <ModuleTabs
      label={t('subnav.label', { defaultValue: 'Alerts views' })}
      className="relative z-10"
      end={end}
      dataAttributes={{ 'data-testid': 'alerts-subnav' }}
      items={tabs.map((tab) => ({
        key: tab.to,
        to: tab.to,
        group: tab.group,
        label: t(tab.labelKey, { defaultValue: tab.fallback }),
      }))}
    />
  );
}

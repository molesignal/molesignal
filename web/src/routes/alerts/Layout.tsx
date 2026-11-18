import { useTranslation } from 'react-i18next';
import { useLocation } from 'react-router-dom';

import { useOpenablePages } from '@/product/moduleAccess';
import { ManagementNav } from '@/shell/ManagementNav';
import { useAlertsSidebarStore } from '@/stores/sidebarCollapse';

interface AlertPage {
  to: string;
  labelKey: string;
  fallback: string;
}

/**
 * The module's pages by what they are for, in the order an alert is met: what is happening and
 * what came of it, what finds it, what keeps the noise down, who is told, and what follows.
 */
const ALERT_GROUPS: ReadonlyArray<{ key: string; pages: readonly AlertPage[] }> = [
  {
    key: 'respond',
    pages: [
      { to: '/alerts/incidents', labelKey: 'subnav.incidents', fallback: 'Incidents' },
      { to: '/alerts/history', labelKey: 'subnav.history', fallback: 'History' },
      { to: '/alerts/postmortems', labelKey: 'subnav.postmortems', fallback: 'Postmortems' },
      { to: '/alerts/insights', labelKey: 'subnav.insights', fallback: 'Insights' },
    ],
  },
  {
    key: 'detect',
    pages: [
      { to: '/alerts/rules', labelKey: 'subnav.rules', fallback: 'Rules' },
      { to: '/alerts/discovery', labelKey: 'subnav.discovery', fallback: 'Discovery' },
      { to: '/alerts/sources', labelKey: 'subnav.sources', fallback: 'Sources' },
    ],
  },
  {
    key: 'noise',
    pages: [
      { to: '/alerts/semantic-groups', labelKey: 'subnav.groups', fallback: 'Groups' },
      { to: '/alerts/inhibitions', labelKey: 'subnav.inhibitions', fallback: 'Inhibition' },
      { to: '/alerts/silences', labelKey: 'subnav.silences', fallback: 'Silences' },
    ],
  },
  {
    key: 'notify',
    pages: [
      { to: '/alerts/routing', labelKey: 'subnav.routing', fallback: 'Routing' },
      { to: '/alerts/escalations', labelKey: 'subnav.escalations', fallback: 'Escalations' },
      { to: '/alerts/schedules', labelKey: 'subnav.schedules', fallback: 'On-call schedules' },
    ],
  },
  {
    key: 'follow_up',
    pages: [
      { to: '/alerts/remediation', labelKey: 'subnav.remediation', fallback: 'Remediation' },
      { to: '/alerts/tickets', labelKey: 'subnav.tickets', fallback: 'Tickets' },
    ],
  },
];

const ALERT_TABS = ALERT_GROUPS.flatMap((group) => group.pages);

/** Where the module opens, for whoever may read the incidents, and failing that its other pages. */
export const ALERT_PAGES = [
  '/alerts/incidents',
  ...ALERT_TABS.map((tab) => tab.to),
] as const;

/**
 * The module's pages in a menu beside the page, grouped by what they are for, as the pages of
 * Access are: fifteen tabs in one row read as a jumble of what is watched and what is set up.
 * A page the reader may not open is not offered.
 */
export function AlertsNav() {
  const { t } = useTranslation('alerts');
  const { pathname } = useLocation();
  const fold = useAlertsSidebarStore((state) => state.toggle);
  const openable = new Set(useOpenablePages(ALERT_TABS, (tab) => tab.to).map((tab) => tab.to));
  const groups = ALERT_GROUPS.map((group) => ({
    key: group.key,
    label: t(`subnav.group_${group.key}`),
    sections: group.pages
      .filter((page) => openable.has(page.to))
      .map((page) => ({ to: page.to, label: t(page.labelKey, { defaultValue: page.fallback }) })),
  })).filter((group) => group.sections.length > 0);

  return (
    <div data-testid="alerts-subnav">
      <ManagementNav
        ariaLabel={t('subnav.label', { defaultValue: 'Alerts views' })}
        currentPath={pathname}
        groups={groups}
        searchPlaceholder={t('subnav.search_placeholder')}
        searchAriaLabel={t('subnav.search_aria')}
        noResultsLabel={t('subnav.no_results')}
        collapseGroupLabel={(group) => t('subnav.collapse_group', { group })}
        expandGroupLabel={(group) => t('subnav.expand_group', { group })}
        collapsibleGroups={false}
        onCollapse={fold}
        collapseNavigationLabel={t('subnav.collapse_navigation')}
        mobilePresentation="drawer"
        mobileTriggerLabel={t('subnav.open_navigation')}
      />
    </div>
  );
}

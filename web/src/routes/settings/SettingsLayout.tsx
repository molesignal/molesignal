import { Settings as SettingsIcon } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Outlet, useLocation } from 'react-router-dom';

import {
  canAccessProductPath,
  useProductAccess,
} from '@/product/access';
import { ManagementPage } from '@/product/templates';
import { cn } from '@/shell/lib/cn';
import { ManagementNav } from '@/shell/ManagementNav';
import { useSettingsSidebarStore } from '@/stores/useSettingsSidebarStore';

import {
  SettingsSaveStatusIndicator,
  SettingsSaveStatusProvider,
} from './SettingsSaveStatus';

interface Section {
  to: string;
  key: string;
  contentWidth: 'page' | 'form' | 'list' | 'table';
}

interface SectionGroup {
  key:
    | 'group_account'
    | 'group_organization'
    | 'group_notify'
    | 'group_data_plane'
    | 'group_security'
    | 'group_ml_ops'
    | 'group_platform';
  sections: Section[];
}

const GROUPS: SectionGroup[] = [
  {
    key: 'group_account',
    sections: [
      {
        to: '/account/billing',
        key: 'account_billing',
        contentWidth: 'page',
      },
    ],
  },
  {
    key: 'group_organization',
    sections: [
      { to: '/settings/general', key: 'general', contentWidth: 'form' },
    ],
  },
  {
    key: 'group_notify',
    sections: [
      {
        to: '/settings/notify/connectors',
        key: 'notify_connectors',
        contentWidth: 'table',
      },
      {
        to: '/settings/notify/users',
        key: 'notify_users',
        contentWidth: 'table',
      },
      {
        to: '/settings/notify/policies',
        key: 'notify_policies',
        contentWidth: 'table',
      },
      {
        to: '/settings/notify/templates',
        key: 'notify_templates',
        contentWidth: 'table',
      },
      {
        to: '/settings/notify/defaults',
        key: 'notify_defaults',
        contentWidth: 'table',
      },
      {
        to: '/settings/notify/deliveries',
        key: 'notify_deliveries',
        contentWidth: 'table',
      },
    ],
  },
  {
    key: 'group_data_plane',
    sections: [
      {
        to: '/settings/client_ip',
        key: 'client_ip',
        contentWidth: 'form',
      },
      { to: '/settings/nodes', key: 'nodes', contentWidth: 'table' },
      { to: '/settings/correlation', key: 'correlation', contentWidth: 'table' },
    ],
  },
  {
    key: 'group_security',
    sections: [
      { to: '/settings/cipher_keys', key: 'cipher_keys', contentWidth: 'table' },
      { to: '/settings/regex_patterns', key: 'regex_patterns', contentWidth: 'table' },
      { to: '/settings/field_masking', key: 'field_masking', contentWidth: 'table' },
      {
        to: '/settings/domain_management',
        key: 'domain_management',
        contentWidth: 'table',
      },
      { to: '/settings/audit', key: 'audit', contentWidth: 'table' },
    ],
  },
  {
    key: 'group_ml_ops',
    sections: [
      { to: '/settings/model_pricing', key: 'model_pricing', contentWidth: 'table' },
      {
        to: '/settings/query_management',
        key: 'query_management',
        contentWidth: 'table',
      },
    ],
  },
  {
    key: 'group_platform',
    sections: [
      {
        to: '/settings/organization_management',
        key: 'organization_management',
        contentWidth: 'table',
      },
      { to: '/settings/license', key: 'license', contentWidth: 'form' },
      {
        to: '/settings/billing',
        key: 'billing_integration',
        contentWidth: 'form',
      },
    ],
  },
];

const CONTENT_WIDTH_CLASS: Record<Section['contentWidth'], string> = {
  page: 'max-w-[1680px]',
  form: 'max-w-[1280px]',
  list: 'max-w-[1600px]',
  table: 'max-w-[1920px]',
};

/**
 * Routed Settings hub. Mounts at `/settings` with `<Outlet />` rendering
 * the per-section sub-page; `/settings` itself redirects to
 * `/settings/general` via the router config.
 */
export function SettingsLayout() {
  return (
    <SettingsSaveStatusProvider>
      <SettingsLayoutFrame />
    </SettingsSaveStatusProvider>
  );
}

function SettingsLayoutFrame() {
  const { t } = useTranslation('settings-admin');
  const { pathname } = useLocation();
  const sidebarCollapsed = useSettingsSidebarStore((state) => state.collapsed);
  const toggleSidebar = useSettingsSidebarStore((state) => state.toggle);
  const current = GROUPS.flatMap((g) => g.sections).find((s) => s.to === pathname);
  const contentWidth =
    current?.contentWidth ?? 'page';
  return (
    <ManagementPage
      appearance="surface"
      title={t('title')}
      subtitle={t('subtitle') as string}
      toolbar={<SettingsSaveStatusIndicator />}
      breadcrumbs={null}
      backTo={null}
      sections={<SettingsNav onCollapse={toggleSidebar} />}
      sectionNavigation={{
        collapsed: sidebarCollapsed,
        onExpand: toggleSidebar,
        expandLabel: t('nav.expand_navigation'),
      }}
      headerClassName="shrink-0"
      headerCompact
      headerIcon={SettingsIcon}
      bodyClassName="mx-auto w-full max-w-[2200px] gap-[12px] [&_[data-product-state]]:border-0"
    >
      <div className="min-w-0">
        <div
          data-settings-content-width={contentWidth}
          className={cn(
            'w-full min-w-0',
            '[&>[data-admin-page-header]]:min-h-0 [&>[data-admin-page-header]]:border-b-0 [&>[data-admin-page-header]]:bg-transparent [&>[data-admin-page-header]]:px-0 [&>[data-admin-page-header]]:py-0',
            CONTENT_WIDTH_CLASS[contentWidth],
          )}
        >
          <Outlet />
        </div>
      </div>
    </ManagementPage>
  );
}

function SettingsNav({ onCollapse }: { onCollapse: () => void }) {
  const { t } = useTranslation('settings-admin');
  const { pathname } = useLocation();
  const access = useProductAccess();
  const groups = GROUPS.map((group) => ({
    key: group.key,
    label: t(`nav.${group.key}`),
    sections: group.sections
      .filter((section) => canAccessProductPath(section.to, access))
      .map((section) => ({
        to: section.to,
        label: t(`nav.${section.key}`),
      })),
  })).filter((group) => group.sections.length > 0);

  return (
    <ManagementNav
      ariaLabel={t('title')}
      currentPath={pathname}
      groups={groups}
      searchPlaceholder={t('nav.search_placeholder')}
      searchAriaLabel={t('nav.search_aria')}
      noResultsLabel={t('nav.no_results')}
      collapseGroupLabel={(group) => t('nav.collapse_group', { group })}
      expandGroupLabel={(group) => t('nav.expand_group', { group })}
      collapsibleGroups={false}
      onCollapse={onCollapse}
      collapseNavigationLabel={t('nav.collapse_navigation')}
      mobilePresentation="drawer"
      mobileTriggerLabel={t('nav.open_navigation')}
    />
  );
}

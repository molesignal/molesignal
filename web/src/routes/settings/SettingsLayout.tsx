import { useTranslation } from 'react-i18next';
import { Outlet, useLocation } from 'react-router-dom';

import {
  canAccessProductPath,
  useProductAccess,
} from '@/product/access';
import { ManagementPage } from '@/product/templates';
import { cn } from '@/shell/lib/cn';
import { ManagementNav } from '@/shell/ManagementNav';

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
    | 'group_notify'
    | 'group_data_plane'
    | 'group_security'
    | 'group_ml_ops';
  sections: Section[];
}

const GROUPS: SectionGroup[] = [
  {
    key: 'group_account',
    sections: [
      { to: '/settings/general', key: 'general', contentWidth: 'form' },
      {
        to: '/settings/organization_management',
        key: 'organization_management',
        contentWidth: 'table',
      },
      { to: '/settings/license', key: 'license', contentWidth: 'form' },
      { to: '/settings/billing', key: 'billing', contentWidth: 'form' },
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
];

const CONTENT_WIDTH_CLASS: Record<Section['contentWidth'], string> = {
  page: 'max-w-[1280px]',
  form: 'max-w-[1280px]',
  list: 'max-w-[1080px]',
  table: 'max-w-[1440px]',
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
  const access = useProductAccess();
  // 子页面包屑：从已有的 GROUPS 定义推导「设置 > 当前页 + 返回」，统一所有子页的层级
  // 感（此前只有 ia.ts 里登记过的 license/organization 才有面包屑）。general 是设置
  // 入口，自身不加面包屑。
  const current = GROUPS.flatMap((g) => g.sections).find((s) => s.to === pathname);
  const crumbs =
    current &&
    current.key !== 'general' &&
    canAccessProductPath('/settings/general', access)
      ? [
          { labelKey: 'settings', label: t('title'), to: '/settings/general' },
          { labelKey: current.key, label: t(`nav.${current.key}`) },
        ]
      : null;
  const contentWidth =
    current?.contentWidth ?? 'page';
  return (
    <ManagementPage
      title={t('title')}
      subtitle={t('subtitle') as string}
      toolbar={<SettingsSaveStatusIndicator />}
      breadcrumbs={crumbs}
      backTo={crumbs ? '/settings/general' : null}
      sections={<SettingsNav />}
    >
      <div className="min-w-0">
        <div
          data-settings-content-width={contentWidth}
          className={cn(
            'ml-0 mr-auto w-full min-w-0',
            CONTENT_WIDTH_CLASS[contentWidth],
          )}
        >
          <Outlet />
        </div>
      </div>
    </ManagementPage>
  );
}

function SettingsNav() {
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
    />
  );
}

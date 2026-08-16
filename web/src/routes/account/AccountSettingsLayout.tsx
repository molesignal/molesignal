import { useTranslation } from 'react-i18next';
import { Outlet, useLocation } from 'react-router-dom';

import { ManagementPage } from '@/product/templates';
import { ManagementNav } from '@/shell/ManagementNav';

const GROUPS = [
  {
    key: 'personal',
    items: [
      { key: 'profile', to: '/account/settings/profile' },
      {
        key: 'preferences',
        to: '/account/settings/preferences',
      },
      {
        key: 'notify',
        to: '/account/settings/notify',
      },
    ],
  },
  {
    key: 'access',
    items: [
      { key: 'security', to: '/account/settings/security' },
      {
        key: 'sessions',
        to: '/account/settings/sessions',
      },
    ],
  },
  {
    key: 'workspace',
    items: [
      {
        key: 'workspace_identity',
        to: '/account/settings/workspace',
      },
    ],
  },
] as const;

export function AccountSettingsLayout() {
  const { t } = useTranslation('account');
  return (
    <ManagementPage
      title={t('title')}
      subtitle={t('subtitle')}
      breadcrumbs={null}
      backTo={null}
      sections={<AccountNav />}
      headerClassName="gap-2 py-3.5"
      bodyClassName="mx-auto w-full max-w-[2200px] gap-6 [&_[data-product-state]]:rounded-none [&_[data-product-state]]:border-0 [&_[data-product-state]]:bg-transparent"
    >
      <Outlet />
    </ManagementPage>
  );
}

function AccountNav() {
  const { t } = useTranslation('account');
  const { pathname } = useLocation();
  const groups = GROUPS.map((group) => ({
    key: group.key,
    label: t(`nav.groups.${group.key}`),
    sections: group.items.map((item) => ({
      to: item.to,
      label: t(`nav.items.${item.key}`),
    })),
  }));

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
      mobilePresentation="drawer"
      mobileTriggerLabel={t('nav.open_navigation')}
    />
  );
}

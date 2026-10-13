import { useTranslation } from 'react-i18next';
import { Outlet, useLocation } from 'react-router-dom';

import { ManagementPage } from '@/product/templates';
import { ManagementNav } from '@/shell/ManagementNav';
import { useSubnavStore } from '@/stores/useSubnavStore';

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
  const collapseMenu = useSubnavStore((state) => state.toggle);
  return (
    <ManagementPage
      title={t('title')}
      subtitle={t('subtitle')}
      breadcrumbs={null}
      backTo={null}
      menuOwnsCollapse
      sections={<AccountNav onCollapse={collapseMenu} />}
      bodyClassName="mx-auto w-full max-w-[2200px] gap-6 [&_[data-product-state]]:rounded-none [&_[data-product-state]]:border-0 [&_[data-product-state]]:bg-transparent"
    >
      <Outlet />
    </ManagementPage>
  );
}

function AccountNav({ onCollapse }: { onCollapse: () => void }) {
  const { t } = useTranslation('account');
  const { t: tShell } = useTranslation('shell');
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
      onCollapse={onCollapse}
      collapseNavigationLabel={tShell('subnav.collapse')}
      mobilePresentation="drawer"
      mobileTriggerLabel={t('nav.open_navigation')}
    />
  );
}

import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Navigate, Outlet, useLocation } from 'react-router-dom';

import { PageHeader as AdminPageHeader } from '@/admin';
import {
  canAccessProductPath,
  useProductAccess,
} from '@/product/access';
import { ProductState, type ProductStateProps } from '@/product/states';
import { ManagementPage } from '@/product/templates';
import { ManagementNav } from '@/shell/ManagementNav';

const IAM_GROUPS = [
  {
    key: 'group_members',
    sections: [
      { to: '/iam/users', key: 'users' },
      { to: '/iam/invitations', key: 'invitations' },
      { to: '/iam/approvals', key: 'approvals' },
    ],
  },
  {
    key: 'group_organization',
    sections: [
      { to: '/iam/organizations', key: 'organizations' },
      { to: '/iam/teams', key: 'teams' },
    ],
  },
  {
    key: 'group_permissions',
    sections: [
      { to: '/iam/roles', key: 'roles' },
      { to: '/iam/groups', key: 'groups' },
      { to: '/iam/quota', key: 'quota' },
    ],
  },
  {
    key: 'group_authentication',
    sections: [
      { to: '/iam/service-accounts', key: 'service_accounts' },
      { to: '/iam/sso', key: 'sso' },
      { to: '/iam/email-domains', key: 'email_domains' },
    ],
  },
];

export function IamLayout() {
  const { t } = useTranslation('iam');
  const { pathname } = useLocation();
  const access = useProductAccess();
  const iamSections = IAM_GROUPS.flatMap((group) => group.sections);
  const landingPath =
    iamSections.find((section) => canAccessProductPath(section.to, access))
      ?.to ?? '/account/settings/profile';
  // 子页面包屑：从分组导航推导「身份与访问 > 当前页 + 返回」，与 Settings 子页一致。
  // users 是 IAM 落地页，自身不加面包屑。
  const current = iamSections.find((s) => s.to === pathname);
  const crumbs =
    current && current.to !== landingPath
      ? [
          { labelKey: 'iam', label: t('title'), to: landingPath },
          { labelKey: current.key, label: t(`nav.${current.key}`) },
        ]
      : null;
  return (
    <ManagementPage
      title={t('title')}
      subtitle={t('subtitle') as string}
      breadcrumbs={crumbs}
      backTo={crumbs ? landingPath : null}
      sections={<IamNav />}
    >
      <div className="ml-0 mr-auto w-full min-w-0 max-w-[1440px]">
        <Outlet />
      </div>
    </ManagementPage>
  );
}

export function IamIndexRedirect() {
  const access = useProductAccess();
  const landingPath = IAM_GROUPS.flatMap((group) => group.sections).find(
    (section) => canAccessProductPath(section.to, access),
  )?.to;
  return (
    <Navigate
      to={landingPath ?? '/account/settings/profile'}
      replace
    />
  );
}

function IamNav() {
  const { t } = useTranslation('iam');
  const { pathname } = useLocation();
  const access = useProductAccess();
  const groups = IAM_GROUPS.map((group) => ({
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

export function IamListPage({
  title,
  subtitle,
  toolbar,
  state,
  children,
}: {
  title: React.ReactNode;
  subtitle?: string | undefined;
  toolbar?: React.ReactNode | undefined;
  state?: ProductStateProps | null | undefined;
  children?: React.ReactNode | undefined;
}) {
  return (
    <>
      <AdminPageHeader title={title} subtitle={subtitle} actions={toolbar} />
      <div className="p-4 lg:p-6">
        {state ? <ProductState {...state} /> : children}
      </div>
    </>
  );
}

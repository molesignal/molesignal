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
import { cn } from '@/shell/lib/cn';
import { ManagementNav } from '@/shell/ManagementNav';
import { useIamSidebarStore } from '@/stores/useIamSidebarStore';

interface IamSection {
  to: string;
  key: string;
  contentWidth: 'page' | 'form' | 'list' | 'table';
}

interface IamSectionGroup {
  key: string;
  sections: IamSection[];
}

const IAM_GROUPS: IamSectionGroup[] = [
  {
    key: 'group_members',
    sections: [
      { to: '/iam/users', key: 'users', contentWidth: 'table' },
      { to: '/iam/invitations', key: 'invitations', contentWidth: 'table' },
      { to: '/iam/approvals', key: 'approvals', contentWidth: 'table' },
    ],
  },
  {
    key: 'group_organization',
    sections: [
      { to: '/iam/organizations', key: 'organizations', contentWidth: 'table' },
      { to: '/iam/teams', key: 'teams', contentWidth: 'table' },
    ],
  },
  {
    key: 'group_permissions',
    sections: [
      { to: '/iam/roles', key: 'roles', contentWidth: 'table' },
      { to: '/iam/groups', key: 'groups', contentWidth: 'table' },
      { to: '/iam/quota', key: 'quota', contentWidth: 'list' },
    ],
  },
  {
    key: 'group_authentication',
    sections: [
      {
        to: '/iam/service-accounts',
        key: 'service_accounts',
        contentWidth: 'table',
      },
      { to: '/iam/sso', key: 'sso', contentWidth: 'table' },
      {
        to: '/iam/email-domains',
        key: 'email_domains',
        contentWidth: 'table',
      },
    ],
  },
];

const CONTENT_WIDTH_CLASS: Record<IamSection['contentWidth'], string> = {
  page: 'max-w-[1120px]',
  form: 'max-w-[1120px]',
  list: 'max-w-[1080px]',
  table: 'max-w-[1440px]',
};

export function IamLayout() {
  const { t } = useTranslation('iam');
  const { pathname } = useLocation();
  const access = useProductAccess();
  const sidebarCollapsed = useIamSidebarStore((state) => state.collapsed);
  const toggleSidebar = useIamSidebarStore((state) => state.toggle);
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
  const contentWidth = current?.contentWidth ?? 'page';
  return (
    <ManagementPage
      title={t('title')}
      subtitle={t('subtitle') as string}
      breadcrumbs={crumbs}
      backTo={crumbs ? landingPath : null}
      sections={<IamNav onCollapse={toggleSidebar} />}
      sectionNavigation={{
        collapsed: sidebarCollapsed,
        onExpand: toggleSidebar,
        expandLabel: t('nav.expand_navigation'),
      }}
      headerClassName="gap-2 py-3.5"
      bodyClassName="mx-auto w-full max-w-[1440px] gap-8"
    >
      <div className="min-w-0">
        <div
          data-iam-content-width={contentWidth}
          className={cn(
            'mx-auto w-full min-w-0',
            CONTENT_WIDTH_CLASS[contentWidth],
          )}
        >
          <Outlet />
        </div>
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

function IamNav({ onCollapse }: { onCollapse: () => void }) {
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
      collapsibleGroups={false}
      onCollapse={onCollapse}
      collapseNavigationLabel={t('nav.collapse_navigation')}
      mobilePresentation="drawer"
      mobileTriggerLabel={t('nav.open_navigation')}
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
      <AdminPageHeader
        title={title}
        subtitle={subtitle}
        actions={toolbar}
        className="bg-transparent"
      />
      <div className="p-4 lg:p-6">
        {state ? <ProductState {...state} /> : children}
      </div>
    </>
  );
}

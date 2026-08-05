import {
  Bell,
  Building2,
  KeyRound,
  MonitorSmartphone,
  SlidersHorizontal,
  UserRound,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { NavLink, Outlet } from 'react-router-dom';

import { ManagementPage } from '@/product/templates';
import { cn } from '@/shell/lib/cn';

const GROUPS = [
  {
    key: 'personal',
    items: [
      { key: 'profile', to: '/account/settings/profile', icon: UserRound },
      {
        key: 'preferences',
        to: '/account/settings/preferences',
        icon: SlidersHorizontal,
      },
      {
        key: 'notify',
        to: '/account/settings/notify',
        icon: Bell,
      },
    ],
  },
  {
    key: 'access',
    items: [
      { key: 'security', to: '/account/settings/security', icon: KeyRound },
      {
        key: 'sessions',
        to: '/account/settings/sessions',
        icon: MonitorSmartphone,
      },
    ],
  },
  {
    key: 'workspace',
    items: [
      {
        key: 'workspace_identity',
        to: '/account/settings/workspace',
        icon: Building2,
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
    >
      <Outlet />
    </ManagementPage>
  );
}

function AccountNav() {
  const { t } = useTranslation('account');
  return (
    <nav
      aria-label={t('title')}
      className="sticky top-4 max-h-[calc(100vh-var(--topbar-h)-80px)] overflow-y-auto border-r border-bd-0 pr-4"
    >
      {GROUPS.map((group, groupIndex) => (
        <div key={group.key} className="mb-5 last:mb-0">
          <div
            className={cn(
              'mb-1 flex items-center px-2.5 font-sans text-xs font-strong text-tx-3',
              groupIndex === 0 && 'min-h-9 pr-10',
            )}
          >
            {t(`nav.groups.${group.key}`)}
          </div>
          <div className="space-y-0.5">
            {group.items.map((item) => {
              const Icon = item.icon;
              return (
                <NavLink
                  key={item.key}
                  to={item.to}
                  className={({ isActive }) =>
                    cn(
                      'relative flex min-h-9 items-center gap-2.5 rounded-md px-2.5 font-sans text-xs font-strong text-tx-1 hover:bg-bg-3 hover:text-tx-0',
                      'focus-visible:bg-bg-3 focus-visible:text-tx-0',
                      isActive &&
                        'bg-indigo-dim text-indigo-soft before:absolute before:-left-1 before:top-1/2 before:h-5 before:w-0.5 before:-translate-y-1/2 before:rounded-r before:bg-indigo',
                    )
                  }
                >
                  <Icon className="h-3.5 w-3.5 shrink-0" />
                  <span className="truncate">{t(`nav.items.${item.key}`)}</span>
                </NavLink>
              );
            })}
          </div>
        </div>
      ))}
    </nav>
  );
}

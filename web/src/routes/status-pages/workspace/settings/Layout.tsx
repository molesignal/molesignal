import { useTranslation } from 'react-i18next';
import { NavLink, Outlet } from 'react-router-dom';

import { cn } from '@/shell/lib/cn';
import { PageBody } from '@/shell/PageHeader';

import { StatusPageCanvas } from '../CardlessSurface';
import { useStatusPageWorkspace } from '../Layout';

const SECTIONS = ['general', 'branding', 'localization', 'domain-access', 'automation'] as const;

export function StatusPageSettingsLayout() {
  const { t } = useTranslation('status-pages');
  const workspace = useStatusPageWorkspace();

  return (
    <PageBody className="space-y-0 pb-4 pt-2 lg:pb-6 lg:pt-2">
      <StatusPageCanvas className="grid grid-cols-1 lg:grid-cols-[220px_minmax(0,1fr)]">
        <nav
          aria-label={t('settings.label')}
          className="h-fit border-b border-bd-0 p-2 lg:border-b-0 lg:border-r"
        >
          {SECTIONS.map((section) => (
            <NavLink
              key={section}
              to={`/status-pages/${workspace.pageId}/settings/${section}`}
              className={({ isActive }) =>
                cn(
                  'flex min-h-11 items-center rounded-md px-3 text-xs font-strong transition-colors lg:min-h-9',
                  isActive ? 'bg-bg-3 text-tx-0' : 'text-tx-2 hover:bg-bg-2 hover:text-tx-0',
                )
              }
            >
              {t(`settings.sections.${section}`)}
            </NavLink>
          ))}
        </nav>
        <div data-status-page-settings-content className="min-w-0 px-4 lg:px-6">
          <Outlet context={workspace} />
        </div>
      </StatusPageCanvas>
    </PageBody>
  );
}

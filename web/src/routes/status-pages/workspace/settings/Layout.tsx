import { useTranslation } from 'react-i18next';
import { NavLink, Outlet } from 'react-router-dom';

import { cn } from '@/shell/lib/cn';
import { PageBody } from '@/shell/PageHeader';

import { useStatusPageWorkspace } from '../Layout';

const SECTIONS = ['general', 'branding', 'localization', 'domain-access'] as const;

export function StatusPageSettingsLayout() {
  const { t } = useTranslation('status-pages');
  const workspace = useStatusPageWorkspace();

  return (
    <PageBody className="grid gap-5 lg:grid-cols-[220px_minmax(0,1fr)]">
      <nav aria-label={t('settings.label')} className="h-fit rounded-lg border border-bd-0 bg-bg-1 p-2">
        {SECTIONS.map((section) => (
          <NavLink
            key={section}
            to={`/status-pages/${workspace.pageId}/settings/${section}`}
            className={({ isActive }) =>
              cn(
                'flex min-h-9 items-center rounded-md px-3 text-xs font-strong transition-colors',
                isActive ? 'bg-bg-3 text-tx-0' : 'text-tx-2 hover:bg-bg-2 hover:text-tx-0',
              )
            }
          >
            {t(`settings.sections.${section}`)}
          </NavLink>
        ))}
      </nav>
      <div className="min-w-0">
        <Outlet context={workspace} />
      </div>
    </PageBody>
  );
}

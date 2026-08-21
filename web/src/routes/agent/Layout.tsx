import { useQuery } from '@tanstack/react-query';
import {
  Activity,
  Bot,
  ClipboardCheck,
  ListTodo,
  MessageSquareText,
  Settings2,
  Workflow,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link, NavLink, Outlet } from 'react-router-dom';

import * as agentApi from '@/api/agent';
import { cn } from '@/shell/lib/cn';
import {
  surfacePageRootClass,
  surfaceModuleNavigationActiveClass,
  surfaceModuleNavigationClass,
  surfaceModuleNavigationItemClass,
  surfaceModuleNavigationRowClass,
  SurfacePageHeader,
} from '@/shell/SurfaceWorkbench';

const MODULE_NAV = [
  { to: '/agent/chat', key: 'nav.chat', icon: MessageSquareText },
  { to: '/agent/investigations', key: 'nav.investigations', icon: ListTodo },
  { to: '/agent/automations', key: 'nav.automations', icon: Workflow },
  { to: '/agent/approvals', key: 'nav.approvals', icon: ClipboardCheck },
  { to: '/agent/executions', key: 'nav.executions', icon: Activity },
  { to: '/agent/settings', key: 'nav.settings', icon: Settings2 },
] as const;

export function AgentLayout() {
  const { t } = useTranslation('agent');
  const overview = useQuery({
    queryKey: ['agent', 'overview'],
    queryFn: agentApi.overview,
    retry: false,
    refetchInterval: 30_000,
  });
  const loadingValue = overview.isPending ? '…' : '—';

  return (
    <section
      data-page-appearance="surface"
      className={cn(
        'flex h-[calc(100vh-var(--topbar-h))] min-h-0 flex-col',
        surfacePageRootClass,
      )}
    >
      <header
        data-testid="agent-module-header"
        className="shrink-0 bg-[var(--page-canvas)]"
      >
        <div data-testid="agent-title-row">
          <SurfacePageHeader
            compact
            moduleIcon={Bot}
            moduleIconTestId="agent-module-icon"
            title="Mole Agent"
            subtitle={t('module_subtitle')}
            toolbar={
              <div className="flex shrink-0 flex-nowrap items-center gap-2" aria-label={t('overview.label')}>
                <StatusTag
                  label={t('overview.active')}
                  value={overview.data?.active_investigations ?? loadingValue}
                  to="/agent/investigations?status=running"
                  active={Boolean(overview.data?.active_investigations)}
                />
                <StatusTag
                  label={t('overview.pending_approvals')}
                  value={overview.data?.pending_approvals ?? loadingValue}
                  to="/agent/approvals?status=pending"
                  attention={Boolean(overview.data?.pending_approvals)}
                />
              </div>
            }
          />
        </div>

        <div className={surfaceModuleNavigationClass}>
          <nav
            aria-label={t('nav.label')}
            className={cn(
              surfaceModuleNavigationRowClass,
              'grid grid-cols-6 gap-0 md:flex md:gap-5',
            )}
          >
            {MODULE_NAV.map((item) => {
              const Icon = item.icon;
              return (
                <NavLink
                  key={item.to}
                  to={item.to}
                  title={t(item.key)}
                  className={({ isActive }) =>
                    cn(
                      surfaceModuleNavigationItemClass,
                      'min-w-0 justify-center gap-2 px-1 text-xs md:w-auto md:px-1',
                      isActive && surfaceModuleNavigationActiveClass,
                    )
                  }
                >
                  <Icon
                    className="h-4 w-4 shrink-0 md:h-3.5 md:w-3.5"
                    strokeWidth={1.8}
                  />
                  <span className="hidden md:inline">{t(item.key)}</span>
                </NavLink>
              );
            })}
          </nav>
        </div>
      </header>

      <div className="min-h-0 flex-1 overflow-hidden px-[20px] pb-[20px]">
        <div
          data-agent-content-surface
          className="h-full min-h-0 overflow-hidden rounded-md bg-[var(--functional-surface)] [box-shadow:var(--shadow-functional-surface)]"
        >
          <Outlet />
        </div>
      </div>
    </section>
  );
}

function StatusTag({
  label,
  value,
  to,
  active = false,
  attention = false,
}: {
  label: string;
  value: number | string;
  to: string;
  active?: boolean;
  attention?: boolean;
}) {
  return (
    <Link
      to={to}
      className="inline-flex h-7 items-center gap-2 rounded-full bg-bg-2 px-2.5 type-caption text-tx-2 transition-colors duration-fast hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0"
    >
      <span
        className={cn(
          'font-mono font-display-strong text-tx-0',
          active && 'text-indigo',
          attention && 'text-orange-soft',
        )}
      >
        {value}
      </span>
      <span>{label}</span>
    </Link>
  );
}

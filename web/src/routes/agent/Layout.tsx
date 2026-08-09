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
    <section className="flex h-[calc(100vh-var(--topbar-h))] min-h-0 flex-col bg-bg-0">
      <header
        data-testid="agent-module-header"
        className="shrink-0 border-b border-bd-0 bg-bg-1"
      >
        <div
          data-testid="agent-title-row"
          className="flex flex-nowrap items-center gap-2 px-4 py-1.5 lg:px-6"
        >
          <div
            data-testid="agent-module-icon"
            className="grid h-7 w-7 shrink-0 place-items-center rounded-md bg-indigo/10 text-indigo"
          >
            <Bot className="h-3.5 w-3.5" strokeWidth={1.8} />
          </div>
          <div className="flex min-w-0 flex-1 items-baseline gap-2 overflow-hidden whitespace-nowrap">
            <h1 className="shrink-0 type-page-title font-sans font-display-strong tracking-[-0.025em] text-tx-0">
              Mole Agent
            </h1>
            <span aria-hidden className="shrink-0 text-tx-3">
              ·
            </span>
            <p className="min-w-0 truncate font-sans type-caption text-tx-2">
              {t('module_subtitle')}
            </p>
          </div>

          <span aria-hidden className="h-5 w-px shrink-0 bg-bd-1" />
          <div
            className="ml-auto flex shrink-0 flex-nowrap items-center gap-2"
            aria-label={t('overview.label')}
          >
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
        </div>

        <nav
          aria-label={t('nav.label')}
          className="grid h-11 grid-cols-6 items-center gap-0 border-t border-bd-0 px-1 md:flex md:gap-1 md:px-3"
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
                    'relative inline-flex h-11 min-w-0 items-center justify-center gap-2 rounded-md px-1 font-sans text-xs font-strong text-tx-2 transition-colors duration-fast ease-default md:h-9 md:w-auto md:px-3',
                    'hover:bg-bg-2 hover:text-tx-0 focus-visible:bg-bg-2 focus-visible:text-tx-0',
                    isActive && 'bg-bg-2 text-tx-0',
                  )
                }
              >
                <Icon className="h-4 w-4 shrink-0 md:h-3.5 md:w-3.5" strokeWidth={1.8} />
                <span className="hidden md:inline">{t(item.key)}</span>
              </NavLink>
            );
          })}
        </nav>
      </header>

      <div className="min-h-0 flex-1 overflow-hidden">
        <Outlet />
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

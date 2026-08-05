import { useQuery } from '@tanstack/react-query';
import {
  Bot,
  CheckCheck,
  ClipboardCheck,
  ListTodo,
  MessageSquareText,
  Settings2,
  Workflow,
} from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link, NavLink, Outlet } from 'react-router-dom';

import * as intelligenceApi from '@/api/intelligence';
import { cn } from '@/shell/lib/cn';

const MODULE_NAV = [
  { to: '/intelligence/chat', key: 'nav.chat', icon: MessageSquareText },
  { to: '/intelligence/investigations', key: 'nav.investigations', icon: ListTodo },
  { to: '/intelligence/automations', key: 'nav.automations', icon: Workflow },
  { to: '/intelligence/approvals', key: 'nav.approvals', icon: ClipboardCheck },
  { to: '/intelligence/executions', key: 'nav.executions', icon: CheckCheck },
  { to: '/intelligence/settings', key: 'nav.settings', icon: Settings2 },
] as const;

export function IntelligenceLayout() {
  const { t } = useTranslation('intelligence');
  const overview = useQuery({
    queryKey: ['intelligence', 'overview'],
    queryFn: intelligenceApi.overview,
    retry: false,
    refetchInterval: 30_000,
  });

  return (
    <section className="flex h-[calc(100vh-var(--topbar-h))] min-h-0 flex-col bg-bg-0">
      <header
        data-testid="intelligence-module-header"
        className="shrink-0 border-b border-bd-0 bg-bg-1"
      >
        <div className="flex items-center gap-3 px-6 py-5">
          <div className="grid h-10 w-10 shrink-0 place-items-center rounded-lg border border-indigo/30 bg-indigo/10">
            <Bot className="h-5 w-5 text-indigo" strokeWidth={1.8} />
          </div>
          <div className="min-w-0">
            <h1 className="type-page-title font-sans font-display-strong tracking-[-0.025em] text-tx-0">
              Mole Intelligence
            </h1>
            <p className="mt-1 hidden truncate font-sans text-sm text-tx-2 sm:block">
              {t('module_subtitle')}
            </p>
          </div>
          {overview.data && (
            <div className="ml-auto hidden items-center gap-4 lg:flex" aria-label={t('overview.label')}>
              <Metric
                label={t('overview.active')}
                value={overview.data.active_investigations}
                to="/intelligence/investigations?status=running"
              />
              <Metric
                label={t('overview.pending_approvals')}
                value={overview.data.pending_approvals}
                alert={overview.data.pending_approvals > 0}
                to="/intelligence/approvals?status=pending"
              />
            </div>
          )}
        </div>

        <nav
          aria-label={t('nav.label')}
          className="grid h-11 grid-cols-6 items-center gap-0 px-1 md:flex md:gap-1 md:px-3"
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
                    'hover:bg-bg-2 hover:text-tx-0 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo/30',
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

function Metric({
  label,
  value,
  alert = false,
  to,
}: {
  label: string;
  value: number;
  alert?: boolean;
  to: string;
}) {
  return (
    <Link
      to={to}
      className="flex items-baseline gap-1.5 rounded-md px-1.5 py-1 hover:bg-bg-2"
    >
      <span className="font-mono text-sm font-strong text-tx-0">{value}</span>
      <span className={cn('font-sans text-xs text-tx-3', alert && 'text-yellow-soft')}>{label}</span>
    </Link>
  );
}

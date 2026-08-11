import {
  Activity,
  Braces,
  CheckCircle2,
  CircleDashed,
  CircleOff,
  Clock3,
  Globe2,
  KeyRound,
  MonitorPlay,
  Network,
  Radar,
  ServerCog,
  Settings,
  ShieldCheck,
  TriangleAlert,
  XCircle,
  type LucideIcon,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { NavLink, useLocation, useNavigate } from 'react-router-dom';

import type { MonitorKind, MonitorState, ProbeOutcome } from '@/api/synthetics';
import { ProductState } from '@/product/states';
import { cn } from '@/shell/lib/cn';
import { PageBody, PageHeader } from '@/shell/PageHeader';

const NAV_ITEMS = [
  ['overview', '/synthetics/overview', Radar],
  ['checks', '/synthetics/checks', Activity],
  ['browser', '/synthetics/browser-tests', MonitorPlay],
  ['api', '/synthetics/api-tests', Braces],
  ['network', '/synthetics/network', Network],
  ['schedules', '/synthetics/schedules', Clock3],
  ['locations', '/synthetics/locations', Globe2],
  ['agents', '/synthetics/agents', ServerCog],
  ['results', '/synthetics/results', CheckCircle2],
  ['assertions', '/synthetics/assertions', ShieldCheck],
  ['variables', '/synthetics/variables', KeyRound],
  ['settings', '/synthetics/settings', Settings],
] as const;

export function SyntheticsNavigation() {
  const { t } = useTranslation('synthetics');
  const location = useLocation();
  const navigate = useNavigate();
  const activePath = NAV_ITEMS.find(
    ([, to]) => location.pathname === to || location.pathname.startsWith(`${to}/`),
  )?.[1];
  return (
    <nav
      aria-label={t('navigation_label')}
      className="relative z-10 -mt-px flex min-h-11 min-w-0 items-center border-b border-bd-0 bg-bg-1 px-3"
    >
      <select
        aria-label={t('navigation_label')}
        value={activePath ?? NAV_ITEMS[0][1]}
        onChange={(event) => navigate(event.target.value)}
        className="h-9 w-full rounded-md border border-bd-0 bg-bg-2 px-3 text-sm font-strong text-tx-0 focus:bg-bg-3 sm:hidden"
      >
        {NAV_ITEMS.map(([key, to]) => (
          <option key={key} value={to}>
            {t(`tabs.${key}`)}
          </option>
        ))}
      </select>
      <div className="hidden min-w-0 flex-1 items-center gap-1 overflow-x-auto overflow-y-hidden sm:flex">
        {NAV_ITEMS.map(([key, to, Icon]) => (
          <NavLink
            key={key}
            to={to}
            className={({ isActive }) =>
              cn(
                'inline-flex h-9 shrink-0 items-center gap-1.5 whitespace-nowrap rounded-md px-3 text-xs font-strong transition-colors duration-fast',
                isActive
                  ? 'bg-bg-3 text-tx-0'
                  : 'text-tx-2 hover:bg-bg-2 hover:text-tx-0 focus-visible:bg-bg-2 focus-visible:text-tx-0',
              )
            }
          >
            <Icon aria-hidden className="h-3.5 w-3.5" />
            {t(`tabs.${key}`)}
          </NavLink>
        ))}
      </div>
    </nav>
  );
}

export function SyntheticsPage({
  title,
  subtitle,
  toolbar,
  children,
  bodyClassName,
}: {
  title: React.ReactNode;
  subtitle?: string;
  toolbar?: React.ReactNode;
  children: React.ReactNode;
  bodyClassName?: string;
}) {
  return (
    <div className="min-h-0 bg-bg-0">
      <PageHeader title={<h1 className="m-0">{title}</h1>} subtitle={subtitle} toolbar={toolbar} />
      <SyntheticsNavigation />
      <PageBody className={cn('space-y-5', bodyClassName)}>{children}</PageBody>
    </div>
  );
}

const STATE_STYLE: Record<MonitorState | ProbeOutcome, { icon: LucideIcon; className: string }> = {
  healthy: { icon: CheckCircle2, className: 'border-green/25 bg-green-dim text-green-soft' },
  degraded: { icon: TriangleAlert, className: 'border-yellow/25 bg-yellow-dim text-yellow-soft' },
  failing: { icon: XCircle, className: 'border-red/25 bg-red-dim text-red-soft' },
  unknown: { icon: CircleDashed, className: 'border-bd-1 bg-bg-3 text-tx-2' },
  skipped: { icon: CircleOff, className: 'border-bd-1 bg-bg-3 text-tx-2' },
};

export function StatePill({ state, compact = false }: { state: MonitorState | ProbeOutcome; compact?: boolean }) {
  const { t } = useTranslation('synthetics');
  const style = STATE_STYLE[state];
  const Icon = style.icon;
  return (
    <span
      className={cn(
        'inline-flex items-center rounded-full border font-strong',
        compact ? 'gap-1 px-1.5 py-0.5 text-type-micro' : 'gap-1.5 px-2 py-1 text-xs',
        style.className,
      )}
    >
      <Icon aria-hidden className={compact ? 'h-3 w-3' : 'h-3.5 w-3.5'} />
      {t(`states.${state}`)}
    </span>
  );
}

const KIND_ICON: Record<MonitorKind, LucideIcon> = {
  http: Braces,
  browser: MonitorPlay,
  tcp: Network,
  dns: Globe2,
  icmp: Radar,
  tls: ShieldCheck,
  grpc: Activity,
  heartbeat: Clock3,
};

export function KindLabel({ kind }: { kind: MonitorKind }) {
  const { t } = useTranslation('synthetics');
  const Icon = KIND_ICON[kind];
  return (
    <span className="inline-flex min-w-0 items-center gap-1.5 text-tx-1">
      <Icon aria-hidden className="h-3.5 w-3.5 shrink-0 text-tx-3" />
      <span className="truncate">{t(`kinds.${kind}`)}</span>
    </span>
  );
}

export function Section({
  title,
  description,
  action,
  children,
  className,
}: {
  title: React.ReactNode;
  description?: React.ReactNode;
  action?: React.ReactNode;
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <section className={cn('overflow-hidden rounded-lg border border-bd-0 bg-bg-1', className)}>
      <div className="flex min-h-14 items-center justify-between gap-4 border-b border-bd-0 px-4 py-3">
        <div className="min-w-0">
          <h2 className="type-section-title font-strong text-tx-0">{title}</h2>
          {description && <p className="mt-0.5 text-xs text-tx-2">{description}</p>}
        </div>
        {action}
      </div>
      {children}
    </section>
  );
}

export function WorkspaceBoundary({
  pending,
  error,
  onRetry,
  children,
}: {
  pending: boolean;
  error: unknown;
  onRetry: () => void;
  children: React.ReactNode;
}) {
  const { t } = useTranslation('synthetics');
  if (pending) return <ProductState variant="loading" title={t('states.loading')} />;
  if (error) {
    return (
      <ProductState
        variant="error"
        title={t('states.load_error')}
        error={error}
        action={
          <button
            type="button"
            onClick={onRetry}
            className="h-9 rounded-md bg-indigo px-3 text-xs font-strong text-white hover:bg-indigo-soft focus-visible:bg-indigo-soft"
          >
            {t('actions.refresh')}
          </button>
        }
      />
    );
  }
  return <>{children}</>;
}

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

import type { KpiStripItem } from '@/admin';
import type { MonitorKind, MonitorState, ProbeOutcome } from '@/api/synthetics';
import { ProductState } from '@/product/states';
import { uiLabelClass } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import {
  surfacePageRootClass,
  surfaceModuleNavigationActiveClass,
  surfaceModuleNavigationClass,
  surfaceModuleNavigationItemClass,
  surfaceModuleNavigationRowClass,
  SurfacePageBody,
  SurfacePageHeader,
} from '@/shell/SurfaceWorkbench';

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
    <div
      className={cn(
        surfaceModuleNavigationClass,
        'relative z-10 flex items-center',
      )}
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
      <nav
        aria-label={t('navigation_label')}
        className={cn(
          surfaceModuleNavigationRowClass,
          'hidden min-w-0 flex-1 sm:flex',
        )}
      >
        {NAV_ITEMS.map(([key, to, Icon]) => (
          <NavLink
            key={key}
            to={to}
            className={({ isActive }) =>
              cn(
                surfaceModuleNavigationItemClass,
                'gap-1.5 whitespace-nowrap',
                isActive
                  ? surfaceModuleNavigationActiveClass
                  : 'text-tx-2',
              )
            }
          >
            <Icon aria-hidden className="h-3.5 w-3.5" />
            {t(`tabs.${key}`)}
          </NavLink>
        ))}
      </nav>
    </div>
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
    <div data-page-appearance="surface" className={surfacePageRootClass}>
      <SurfacePageHeader title={title} subtitle={subtitle} toolbar={toolbar} />
      <SyntheticsNavigation />
      <SurfacePageBody className={cn('space-y-[12px]', bodyClassName)}>{children}</SurfacePageBody>
    </div>
  );
}

const KPI_GRID_CLASS = {
  3: 'sm:grid-cols-3',
  4: 'sm:grid-cols-2 xl:grid-cols-4',
  5: 'sm:grid-cols-2 xl:grid-cols-5',
  6: 'sm:grid-cols-2 xl:grid-cols-3 2xl:grid-cols-6',
} as const;

export function SyntheticsCanvas({
  children,
  className,
}: {
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <div
      data-synthetics-canvas
      className={cn('mx-auto w-full max-w-[2200px] space-y-[12px] bg-[var(--page-canvas)]', className)}
    >
      {children}
    </div>
  );
}

export function SyntheticsKpiBand({
  items,
  columns = 4,
  className,
}: {
  items: readonly KpiStripItem[];
  columns?: keyof typeof KPI_GRID_CLASS;
  className?: string;
}) {
  if (items.length === 0) return null;
  return (
    <section
      data-synthetics-kpis
      className={cn(
        'grid grid-cols-1 gap-[12px]',
        KPI_GRID_CLASS[columns],
        className,
      )}
    >
      {items.map((item, index) => (
        <div
          key={index}
          className="min-h-[92px] min-w-0 rounded-md bg-[var(--functional-surface)] px-4 py-3 [box-shadow:var(--shadow-functional-surface)]"
        >
          <div className={uiLabelClass}>{item.label}</div>
          <div
            className={cn(
              'mt-2 truncate font-sans text-2xl font-display-strong leading-none tracking-[-0.025em] tabular-nums',
              (!item.tone || item.tone === 'neutral') && 'text-tx-0',
              item.tone === 'good' && 'text-green',
              item.tone === 'warn' && 'text-yellow',
              item.tone === 'danger' && 'text-red',
            )}
          >
            {item.value}
          </div>
          {item.sub && (
            <div className="mt-1.5 truncate font-sans text-xs text-tx-2">
              {item.sub}
            </div>
          )}
        </div>
      ))}
    </section>
  );
}

export function SyntheticsListSurface({
  children,
  className,
}: {
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <section
      data-synthetics-list-surface
      className={cn(
        'min-w-0 overflow-hidden rounded-md bg-[var(--functional-surface)] [box-shadow:var(--shadow-functional-surface)]',
        className,
      )}
    >
      {children}
    </section>
  );
}

export function SyntheticsFilterBar({
  children,
  className,
}: {
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <div
      data-synthetics-filter-bar
      className={cn(
        'min-h-12 px-4 py-2',
        className,
      )}
    >
      {children}
    </div>
  );
}

const STATE_STYLE: Record<MonitorState | ProbeOutcome, { icon: LucideIcon; className: string }> = {
  healthy: { icon: CheckCircle2, className: 'border-green/25 bg-green-dim text-green-soft' },
  flaky: { icon: TriangleAlert, className: 'border-orange/25 bg-orange-dim text-orange-soft' },
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
  ssh: KeyRound,
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
  flat = false,
}: {
  title: React.ReactNode;
  description?: React.ReactNode;
  action?: React.ReactNode;
  children: React.ReactNode;
  className?: string;
  flat?: boolean;
}) {
  return (
    <section
      data-synthetics-section={flat ? 'flat' : undefined}
      className={cn(
        'min-w-0 overflow-hidden rounded-md bg-[var(--functional-surface)] [box-shadow:var(--shadow-functional-surface)]',
        className,
      )}
    >
      <div className="flex min-h-14 items-center justify-between gap-4 px-4 py-3">
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
  flat = false,
}: {
  pending: boolean;
  error: unknown;
  onRetry: () => void;
  children: React.ReactNode;
  flat?: boolean;
}) {
  const { t } = useTranslation('synthetics');
  const stateClassName = flat
    ? 'min-h-[240px]'
    : undefined;
  if (pending) {
    return (
      <ProductState
        variant="loading"
        title={t('states.loading')}
        className={stateClassName}
      />
    );
  }
  if (error) {
    return (
      <ProductState
        variant="error"
        title={t('states.load_error')}
        error={error}
        className={stateClassName}
        action={
          <button
            type="button"
            onClick={onRetry}
            className="h-9 rounded-md bg-indigo px-3 text-xs font-strong text-white hover:brightness-90 focus-visible:brightness-90"
          >
            {t('actions.refresh')}
          </button>
        }
      />
    );
  }
  return <>{children}</>;
}

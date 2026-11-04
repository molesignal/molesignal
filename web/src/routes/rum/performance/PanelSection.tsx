import { RefreshCw } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';
import { surfacePanelClass } from '@/shell/SurfaceWorkbench';
import { Skeleton } from '@/shell/ui/skeleton';

/** The aside a view opens beside its table: a group of what the table lists, told in sections. */
export const PANEL_CLASS = cn(surfacePanelClass, 'flex min-w-0 flex-col gap-4 px-4 pb-4 pt-3.5 font-sans');

/**
 * One part of a panel: a small title over what it shows. Until the numbers are there it shows that
 * they are on their way, why they are not there, or that there were none, in the room of one line.
 */
export function Section({
  title,
  count,
  action,
  pending,
  failed,
  unavailable,
  onRetry,
  empty,
  emptyText,
  testId,
  children,
}: {
  title: React.ReactNode;
  /** How many there are of what the section lists, said beside its title. */
  count?: number | undefined;
  /** What the section offers, at the far end of its title row. */
  action?: React.ReactNode;
  pending: boolean;
  failed: boolean;
  /** Why the numbers are not there, when the server says so. */
  unavailable?: 'time_budget' | 'busy' | 'timestamp_masked' | undefined;
  onRetry: () => void;
  empty: boolean;
  emptyText: React.ReactNode;
  testId?: string;
  children: React.ReactNode;
}) {
  const { t } = useTranslation('rum');

  const body = pending ? (
    <div role="status" aria-label={t('explorer.drawer.loading')} className="flex flex-col gap-2">
      <Skeleton className="h-3 w-2/3" />
      <Skeleton className="h-3 w-1/2" />
    </div>
  ) : failed ? (
    <div className="flex items-center justify-between gap-3 text-xs text-tx-2">
      <span>{t('performance.section.failed')}</span>
      <button
        type="button"
        onClick={onRetry}
        className="inline-flex h-7 items-center gap-1.5 rounded-md border-0 bg-[var(--control-surface)] px-2.5 text-xs font-strong text-tx-1 outline-none hover:bg-bg-3 focus-visible:bg-bg-3"
      >
        <RefreshCw aria-hidden className="h-3.5 w-3.5" />
        {t('overview.retry')}
      </button>
    </div>
  ) : unavailable ? (
    <p className="m-0 text-xs text-tx-3">{t(`overview.unavailable.${unavailable}`)}</p>
  ) : empty ? (
    <p className="m-0 text-xs text-tx-3">{emptyText}</p>
  ) : (
    children
  );

  return (
    <section data-rum-panel-section={testId} className="flex min-w-0 flex-col gap-2">
      {count === undefined && !action ? (
        <h2 className="m-0 text-xs font-strong text-tx-3">{title}</h2>
      ) : (
        <div className="flex items-baseline gap-2">
          <h2 className="m-0 text-xs font-strong text-tx-3">{title}</h2>
          {count !== undefined && <span className="text-xs tabular-nums text-tx-3">{count}</span>}
          {action && <span className="ml-auto">{action}</span>}
        </div>
      )}
      {body}
    </section>
  );
}

/** A bar of a distribution: a name, a bar as long as its share, and the share in figures. */
export function ShareBar({
  label,
  share,
  value,
  fill,
  ink,
  labelWidth = '72px',
  mono,
}: {
  label: React.ReactNode;
  /** From 0 to 1. */
  share: number;
  value: React.ReactNode;
  fill: string;
  /** The ink of the name. */
  ink?: string | undefined;
  labelWidth?: string;
  mono?: boolean | undefined;
}) {
  return (
    <div className="grid items-center gap-x-2.5 text-xs" style={{ gridTemplateColumns: `${labelWidth} minmax(0,1fr) 44px` }}>
      <span className={cn('truncate', mono && 'font-mono', ink ?? 'text-tx-1')}>{label}</span>
      <span aria-hidden className="h-2">
        <span className="block h-2 rounded-[2px] opacity-85" style={{ width: `${Math.max(1, Math.min(100, share * 100))}%`, background: fill }} />
      </span>
      <span className="text-right tabular-nums text-tx-0">{value}</span>
    </div>
  );
}

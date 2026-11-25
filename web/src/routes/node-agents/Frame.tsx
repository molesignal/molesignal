import type { TFunction } from 'i18next';
import { KeyRound, ListChecks, Server } from 'lucide-react';
import type * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { NodeStatus, NodeTaskState } from '@/api/nodeAgents';
import { useOpenablePages } from '@/product/moduleAccess';
import type { ProductStateProps } from '@/product/states';
import { ListPage } from '@/product/templates';
import { ChromeButton } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { ModuleTabs } from '@/shell/tabs/ModuleTabs';

/** How often the lists read again: a heartbeat is 30 seconds. */
export const NODE_AGENTS_REFRESH_MILLIS = 15_000;

// The machines, then what they are asked to do, then how a new one joins.
const TABS = [
  ['nodes', '/node-agents/nodes', Server],
  ['tasks', '/node-agents/tasks', ListChecks],
  ['enrollment', '/node-agents/enrollment', KeyRound],
] as const;

/** The module's pages, in the order it opens on the first the reader may see. */
export const NODE_AGENTS_PAGES = TABS.map(([, to]) => to) as [string, ...string[]];

function NodeAgentsNavigation() {
  const { t } = useTranslation('infrastructure');
  const tabs = useOpenablePages(TABS, ([, to]) => to);

  return (
    <ModuleTabs
      label={t('node_agents.navigation_label')}
      className="relative z-10"
      dataAttributes={{ 'data-testid': 'node-agents-subnav' }}
      items={tabs.map(([key, to, icon]) => ({
        key,
        to,
        icon,
        label: t(`node_agents.tabs.${key}`),
      }))}
    />
  );
}

/** The frame every tab stands in: one title, the tabs, and the one thing the open tab adds. */
export function NodeAgentsPage({
  action,
  kpis,
  listHead,
  state,
  children,
}: {
  action?: React.ReactNode | undefined;
  kpis?: { label: string; value: string }[] | undefined;
  listHead?: React.ReactNode | undefined;
  state?: ProductStateProps | null | undefined;
  children?: React.ReactNode | undefined;
}) {
  const { t } = useTranslation('infrastructure');

  return (
    <ListPage
      appearance="surface"
      title={t('node_agents.title')}
      subtitle={t('node_agents.subtitle')}
      subnav={<NodeAgentsNavigation />}
      toolbar={action}
      kpis={kpis}
      kpiLayout="inline"
      listHead={listHead}
      state={state}
    >
      {children}
    </ListPage>
  );
}

/** What a tab shows while its data is on the way or could not be had; nothing once it is in. */
export function useLoadState({
  pending,
  error,
  onRetry,
}: {
  pending: boolean;
  error: unknown;
  onRetry: () => void;
}): ProductStateProps | null {
  const { t } = useTranslation('infrastructure');
  if (pending) return { variant: 'loading' };
  if (!error) return null;

  return {
    variant: 'error',
    title: t('node_agents.load_error'),
    error,
    action: <ChromeButton onClick={onRetry}>{t('list.retry')}</ChromeButton>,
  };
}

const NODE_TONE: Record<NodeStatus, string> = {
  online: 'var(--green)',
  offline: 'var(--red)',
  enrolled: 'var(--indigo)',
  draining: 'var(--blue)',
  drained: 'var(--blue)',
  revoked: 'var(--tx-3)',
};

const TASK_TONE: Record<NodeTaskState, string> = {
  queued: 'var(--tx-3)',
  offered: 'var(--indigo)',
  running: 'var(--blue)',
  succeeded: 'var(--green)',
  failed: 'var(--red)',
  cancelled: 'var(--tx-3)',
  lease_expired: 'var(--yellow)',
  expired: 'var(--yellow)',
  rejected: 'var(--red)',
};

function StatusMark({ tone, label, data }: { tone: string; label: string; data: Record<string, string> }) {
  return (
    <span {...data} className={cn('inline-flex min-w-0 items-center gap-[6px] font-sans text-xs text-tx-1')}>
      <span aria-hidden="true" className="h-[7px] w-[7px] shrink-0 rounded-full" style={{ background: tone }} />
      <span className="truncate">{label}</span>
    </span>
  );
}

export function NodeStatusMark({ status }: { status: NodeStatus }) {
  const { t } = useTranslation('infrastructure');

  return (
    <StatusMark
      tone={NODE_TONE[status]}
      label={t(`node_agents.status.${status}`)}
      data={{ 'data-node-status': status }}
    />
  );
}

export function TaskStateMark({ state }: { state: NodeTaskState }) {
  const { t } = useTranslation('infrastructure');

  return (
    <StatusMark
      tone={TASK_TONE[state]}
      label={t(`node_agents.task_state.${state}`)}
      data={{ 'data-task-state': state }}
    />
  );
}

/** A labelled value in a drawer: the label on the left, the value beside it. */
export function DetailRow({
  label,
  children,
  code = false,
}: {
  label: React.ReactNode;
  children: React.ReactNode;
  code?: boolean;
}) {
  return (
    <div className="grid grid-cols-[minmax(120px,0.4fr)_minmax(0,1fr)] gap-3 px-3 py-2.5 text-xs">
      <div className="text-tx-3">{label}</div>
      <div className={cn('min-w-0 break-all text-tx-1', code && 'font-code')}>{children}</div>
    </div>
  );
}

export function DetailRows({ children }: { children: React.ReactNode }) {
  return <div className="divide-y divide-bd-0 overflow-hidden rounded-md bg-bg-2">{children}</div>;
}

/** A length of time given in minutes, said in the largest whole unit: 90 minutes, 6 hours, 7 days. */
export function durationLabel(t: TFunction<'infrastructure'>, minutes: number): string {
  if (minutes % 1440 === 0) return t('node_agents.duration.days', { count: minutes / 1440 });
  if (minutes % 60 === 0) return t('node_agents.duration.hours', { count: minutes / 60 });
  return t('node_agents.duration.minutes', { count: minutes });
}

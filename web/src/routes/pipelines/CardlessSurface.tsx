import type * as React from 'react';

import {
  Dot,
  Pill,
  uiLabelClass,
  type PillTone,
} from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { TabsTrigger } from '@/shell/ui/tabs';

import type { PipelineSignalType } from './PipelineGraph';
import type { PipelineHealth } from './presentation';

type DotTone = NonNullable<React.ComponentProps<typeof Dot>['tone']>;

const PIPELINE_HEALTH_TONE: Record<
  PipelineHealth,
  { pill: PillTone; dot: DotTone }
> = {
  healthy: { pill: 'green', dot: 'green' },
  running: { pill: 'blue', dot: 'blue' },
  error: { pill: 'red', dot: 'red' },
  paused: { pill: 'dim', dot: 'dim' },
  unknown: { pill: 'yellow', dot: 'yellow' },
  never: { pill: 'yellow', dot: 'yellow' },
};

export const PIPELINE_TYPE_TONE: Record<PipelineSignalType, PillTone> = {
  logs: 'orange',
  metrics: 'blue',
  traces: 'green',
};

export interface PipelineKpiItem {
  label: React.ReactNode;
  value: React.ReactNode;
  note?: React.ReactNode;
  tone?: 'neutral' | 'good' | 'warn' | 'danger';
}

export const pipelineFlatTableClassName =
  'rounded-none border-0 bg-transparent';

export function PipelineKpiBand({
  items,
  className,
}: {
  items: readonly PipelineKpiItem[];
  className?: string;
}) {
  if (items.length === 0) return null;

  return (
    <section
      data-pipeline-kpis
      className={cn(
        'grid grid-cols-1 gap-[12px] sm:grid-cols-2 xl:grid-cols-4',
        className,
      )}
    >
      {items.map((item, index) => (
        <div key={index} className="min-h-[92px] min-w-0 rounded-md bg-[var(--functional-surface)] px-4 py-3 [box-shadow:var(--shadow-functional-surface)]">
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
          {item.note && (
            <div className="mt-1.5 truncate font-sans text-xs text-tx-2">
              {item.note}
            </div>
          )}
        </div>
      ))}
    </section>
  );
}

export function PipelineSection({
  title,
  description,
  actions,
  children,
  className,
  bodyClassName,
  showHeaderDivider = true,
}: {
  title: React.ReactNode;
  description?: React.ReactNode;
  actions?: React.ReactNode;
  children: React.ReactNode;
  className?: string;
  bodyClassName?: string;
  showHeaderDivider?: boolean;
}) {
  return (
    <section
      data-pipeline-section
      className={cn(
        'min-w-0 rounded-md bg-[var(--functional-surface)] p-4 [box-shadow:var(--shadow-functional-surface)]',
        className,
      )}
    >
      <header
        data-header-divider={showHeaderDivider ? 'spaced' : undefined}
        className={cn(
          'flex min-h-[50px] items-center gap-4 py-2.5',
        )}
      >
        <div className="min-w-0 flex-1">
          <h2 className="truncate font-sans text-sm font-display-strong text-tx-0">
            {title}
          </h2>
          {description && (
            <p className="mt-0.5 truncate font-sans text-xs text-tx-2">
              {description}
            </p>
          )}
        </div>
        {actions && <div className="flex shrink-0 items-center gap-1.5">{actions}</div>}
      </header>
      <div className={cn('pt-4', bodyClassName)}>{children}</div>
    </section>
  );
}

export function PipelineConfigSection({
  title,
  children,
}: {
  title: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <section data-pipeline-config-section className="min-w-0 bg-transparent py-2">
      <h3 className="font-sans text-xs font-strong text-tx-2">{title}</h3>
      <div className="mt-3 flex flex-col gap-2">{children}</div>
    </section>
  );
}

export function PipelineConfigValue({ children }: { children: React.ReactNode }) {
  return (
    <div
      data-pipeline-config-value
      className="rounded-md bg-bg-2 px-3 py-2 font-mono text-xs text-tx-1"
    >
      {children}
    </div>
  );
}

export function PipelineTabTrigger({
  value,
  children,
}: {
  value: string;
  children: React.ReactNode;
}) {
  return (
    <TabsTrigger
      value={value}
      className="h-10 rounded-none border-b-[3px] border-transparent bg-transparent px-4 text-xs text-tx-2 shadow-none data-[state=active]:border-indigo data-[state=active]:bg-transparent data-[state=active]:text-tx-0 data-[state=active]:shadow-none"
    >
      {children}
    </TabsTrigger>
  );
}

export function PipelineMetadata({
  label,
  value,
}: {
  label: React.ReactNode;
  value: React.ReactNode;
}) {
  return (
    <span className="inline-flex items-center gap-1.5">
      <span className="text-tx-3">{label}</span>
      <span className="font-strong text-tx-1">{value}</span>
    </span>
  );
}

export function PipelineDetailMetadata({
  health,
  healthLabel,
  typeTone,
  typeLabel,
  items,
  runState,
}: {
  health: PipelineHealth;
  healthLabel: React.ReactNode;
  typeTone: PillTone;
  typeLabel: React.ReactNode;
  items: readonly { label: React.ReactNode; value: React.ReactNode }[];
  runState?: { state: string; label: React.ReactNode } | undefined;
}) {
  const tone = PIPELINE_HEALTH_TONE[health];

  return (
    <div
      data-pipeline-metadata
      className="mb-[12px] flex min-h-12 flex-wrap items-center gap-x-6 gap-y-2 rounded-md bg-[var(--functional-surface)] px-4 py-2.5 font-sans text-xs [box-shadow:var(--shadow-functional-surface)]"
    >
      <Pill tone={tone.pill}>
        <Dot tone={tone.dot} />
        {healthLabel}
      </Pill>
      <Pill tone={typeTone}>{typeLabel}</Pill>
      {items.map((item, index) => (
        <PipelineMetadata key={index} label={item.label} value={item.value} />
      ))}
      {runState && (
        <PipelineRunState state={runState.state} label={runState.label} />
      )}
    </div>
  );
}

export function PipelineRunState({
  state,
  label,
}: {
  state: string;
  label: React.ReactNode;
}) {
  const tone =
    state === 'succeeded'
      ? 'green'
      : state === 'failed'
        ? 'red'
        : state === 'running'
          ? 'blue'
          : 'dim';

  return (
    <span className="inline-flex items-center gap-1.5 text-tx-2">
      <Dot tone={tone} />
      {label}
    </span>
  );
}

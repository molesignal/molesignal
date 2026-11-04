import { ChevronDown, ChevronRight } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';

import { formatTraceDurationNs } from '../duration';
import { groupSpans, type SpanGroup } from '../model/aggregate';
import type { TraceTree } from '../model/tree';
import { TraceOperationName } from '../TraceOperationName';

const GRID = 'grid grid-cols-[minmax(0,1fr)_64px_56px_88px_88px_88px_120px] items-center gap-3';

/**
 * The trace's spans summed by service and operation: how many there were,
 * how long they took on average and in all, and how much of the trace's own
 * work was theirs. An operation opens its spans in the waterfall.
 */
export function SpanGroups({
  tree,
  onOpenOperation,
}: {
  tree: TraceTree;
  /** An operation was picked: its spans are shown where the trace is drawn in full. */
  onOpenOperation: (service: string, operation: string) => void;
}) {
  const { t } = useTranslation('traces');
  const groups = React.useMemo(() => groupSpans(tree), [tree]);
  const [folded, setFolded] = React.useState<ReadonlySet<string>>(() => new Set());

  const toggle = (key: string) =>
    setFolded((current) => {
      const next = new Set(current);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });

  return (
    <div data-testid="trace-span-groups" className="flex h-full min-h-0 flex-col">
      <div className={cn(GRID, 'h-8 shrink-0 border-b border-bd-0 px-3 font-sans text-xs font-strong text-tx-2')}>
        <span>{t('view.groups.name')}</span>
        <span className="text-right">{t('view.groups.spans')}</span>
        <span className="text-right">{t('view.groups.errors')}</span>
        <span className="text-right">{t('view.groups.average')}</span>
        <span className="text-right">{t('view.groups.total')}</span>
        <span className="text-right">{t('view.groups.self')}</span>
        <span>{t('view.groups.share')}</span>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto">
        {groups.map((group) => (
          <div key={group.key} role="group" aria-label={group.service}>
            <GroupLine group={group} open={!folded.has(group.key)} onToggle={() => toggle(group.key)} />
            {!folded.has(group.key) &&
              group.operations.map((operation) => (
                <button
                  key={operation.key}
                  type="button"
                  data-testid="trace-span-group-operation"
                  onClick={() => onOpenOperation(operation.service, operation.operation ?? '')}
                  className={cn(GRID, 'h-8 w-full border-b border-bd-0 pl-9 pr-3 text-left hover:bg-bg-2 focus-visible:bg-bg-2 focus-visible:outline-none')}
                >
                  <TraceOperationName operation={operation.operation ?? ''} className="min-w-0 font-sans text-xs text-tx-1" />
                  <Figures group={operation} />
                </button>
              ))}
          </div>
        ))}
      </div>
    </div>
  );
}

function GroupLine({ group, open, onToggle }: { group: SpanGroup; open: boolean; onToggle: () => void }) {
  const { t } = useTranslation('traces');

  return (
    <button
      type="button"
      aria-expanded={open}
      aria-label={t(open ? 'view.groups.fold' : 'view.groups.unfold', { service: group.service })}
      onClick={onToggle}
      className={cn(GRID, 'h-8 w-full border-b border-bd-0 bg-bg-1 px-3 text-left hover:bg-bg-2 focus-visible:bg-bg-2 focus-visible:outline-none')}
    >
      <span className="flex min-w-0 items-center gap-1.5">
        {open ? <ChevronDown className="h-3.5 w-3.5 shrink-0 text-tx-3" /> : <ChevronRight className="h-3.5 w-3.5 shrink-0 text-tx-3" />}
        <span aria-hidden="true" className="h-2 w-2 shrink-0 rounded-sm" style={{ backgroundColor: group.color }} />
        <span className="truncate font-sans text-xs font-strong text-tx-0">{group.service}</span>
      </span>
      <Figures group={group} />
    </button>
  );
}

function Figures({ group }: { group: SpanGroup }) {
  const percent = Math.round(group.share * 1000) / 10;

  return (
    <>
      <span className="text-right font-sans text-xs tabular-nums text-tx-1">{group.spans}</span>
      <span className={cn('text-right font-sans text-xs tabular-nums', group.errors > 0 ? 'text-[var(--red-ink)]' : 'text-tx-3')}>
        {group.errors}
      </span>
      <span className="text-right font-sans text-xs tabular-nums text-tx-1">{formatTraceDurationNs(group.avgNs)}</span>
      <span className="text-right font-sans text-xs tabular-nums text-tx-1">{formatTraceDurationNs(group.totalNs)}</span>
      <span className="text-right font-sans text-xs tabular-nums text-tx-0">{formatTraceDurationNs(group.selfNs)}</span>
      <span className="flex items-center gap-2">
        <span className="relative h-1.5 flex-1 overflow-hidden rounded-full bg-bg-3">
          <span className="absolute inset-y-0 left-0 rounded-full" style={{ width: `${Math.max(1, percent)}%`, backgroundColor: group.color }} />
        </span>
        <span className="w-10 text-right font-sans text-xs tabular-nums text-tx-1">{percent}%</span>
      </span>
    </>
  );
}

import type { UseQueryResult } from '@tanstack/react-query';
import type { TFunction } from 'i18next';
import { ArrowLeftRight, ExternalLink, X } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { TraceResponse } from '@/api/web';
import { toApiError } from '@/lib/http';
import { useTimeFormatter, zonedMoment } from '@/lib/time';
import { ProductState } from '@/product/states';
import { cn } from '@/shell/lib/cn';
import { ViewTabs } from '@/shell/tabs/ViewTabs';
import { Button } from '@/shell/ui/button';

import { formatTraceDurationNs } from '../duration';
import { useTrace } from '../loader';
import { compareTraces } from '../model/compare';
import { HTTP_STATUS_TEXT, httpStatusOf, httpStatusTone, OUTCOME_FILL, requestOutcome } from '../model/span';
import { buildTraceTree, serviceColor, type TraceTree } from '../model/tree';
import { tracePageHref } from '../pageLink';
import { TraceOperationName } from '../TraceOperationName';
import { type ComparedTrace, traceCompareHref } from './link';
import { CompareTable, MissingCalls, TimeChange } from './Tables';

type CompareTab = 'services' | 'operations' | 'calls';

/**
 * Two traces side by side: each as its head says it, how much longer the
 * second took, the time each service and each operation took in both, and
 * the calls only one of them made. The second is set against the first.
 */
export function TraceCompare({ a, b, className }: { a: ComparedTrace; b: ComparedTrace; className?: string | undefined }) {
  const { t } = useTranslation('traces');
  const left = useTrace(a.traceId, a.window);
  const right = useTrace(b.traceId, b.window);
  const treeA = React.useMemo(() => (left.data ? buildTraceTree(left.data) : null), [left.data]);
  const treeB = React.useMemo(() => (right.data ? buildTraceTree(right.data) : null), [right.data]);
  const comparison = React.useMemo(() => (treeA && treeB ? compareTraces(treeA, treeB) : null), [treeA, treeB]);
  const colors = React.useMemo(() => colorsOf(treeA, treeB), [treeA, treeB]);
  const [tab, setTab] = React.useState<CompareTab>('services');

  const missing = comparison ? comparison.onlyInA.length + comparison.onlyInB.length : 0;
  const tabs = [
    { key: 'services' as const, label: t('compare.tabs.services'), ...(comparison ? { count: comparison.services.length } : {}) },
    { key: 'operations' as const, label: t('compare.tabs.operations'), ...(comparison ? { count: comparison.operations.length } : {}) },
    { key: 'calls' as const, label: t('compare.tabs.calls'), ...(missing > 0 ? { count: missing } : {}) },
  ];

  return (
    <div data-trace-compare="" className={cn('flex h-full min-h-0 w-full min-w-0 flex-col bg-[var(--functional-surface)]', className)}>
      <header className="shrink-0 border-b border-bd-0 px-3 pb-2 pt-2">
        <div className="grid grid-cols-1 gap-2 md:grid-cols-2">
          <Side label="A" trace={a} query={left} tree={treeA} t={t} />
          <Side label="B" trace={b} query={right} tree={treeB} t={t} />
        </div>
        {comparison && treeA && treeB && (
          <p className="mt-2 font-sans text-xs text-tx-1" data-testid="trace-compare-duration">
            {comparison.durationDeltaNs === 0 ? (
              t('compare.duration.same', { duration: formatTraceDurationNs(treeA.totalNs) })
            ) : (
              <>
                <span className="mr-1.5">{t(comparison.durationDeltaNs > 0 ? 'compare.duration.longer' : 'compare.duration.shorter')}</span>
                <TimeChange deltaNs={comparison.durationDeltaNs} baseNs={treeA.totalNs} t={t} />
              </>
            )}
          </p>
        )}
      </header>

      {comparison ? (
        <>
          <div className="shrink-0 border-b border-bd-0 px-3 py-1.5">
            <ViewTabs label={t('compare.tabs.label')} value={tab} onValueChange={setTab} items={tabs} />
          </div>
          <div className="min-h-0 flex-1 overflow-auto">
            {tab === 'services' && <CompareTable rows={comparison.services} colors={colors} kind="services" />}
            {tab === 'operations' && <CompareTable rows={comparison.operations} colors={colors} kind="operations" />}
            {tab === 'calls' && <MissingCalls onlyInA={comparison.onlyInA} onlyInB={comparison.onlyInB} colors={colors} />}
          </div>
        </>
      ) : left.isLoading || right.isLoading ? (
        <ProductState variant="loading" />
      ) : (
        <ProductState variant="empty" title={t('compare.unavailable_title')} description={t('compare.unavailable_description')} />
      )}
    </div>
  );
}

/** The comparison's own controls, for the head of the page it stands in: the two the other way round, or the first alone. */
export function CompareActions({ a, b }: { a: ComparedTrace; b: ComparedTrace }) {
  const { t } = useTranslation('traces');

  return (
    <div className="flex items-center gap-2">
      <Button variant="outline" size="sm" asChild>
        <Link to={traceCompareHref(b, a)} data-testid="trace-compare-swap">
          <ArrowLeftRight aria-hidden="true" className="h-3.5 w-3.5" />
          {t('compare.swap')}
        </Link>
      </Button>
      <Button variant="outline" size="sm" asChild>
        <Link to={tracePageHref(a)} data-testid="trace-compare-stop">
          <X aria-hidden="true" className="h-3.5 w-3.5" />
          {t('compare.stop')}
        </Link>
      </Button>
    </div>
  );
}

/** One of the traces, as its head says it: the request it began with, how it ended, how long it took and when. */
function Side({
  label,
  trace,
  query,
  tree,
  t,
}: {
  label: 'A' | 'B';
  trace: ComparedTrace;
  query: UseQueryResult<TraceResponse>;
  tree: TraceTree | null;
  t: TFunction<'traces'>;
}) {
  const fmt = useTimeFormatter();
  const status = tree ? httpStatusOf(tree.root.span) : null;
  const start = tree ? zonedMoment(Math.floor(tree.startNs / 1_000_000), fmt.tz) : null;

  return (
    <section aria-label={t('compare.side', { side: label })} data-compare-side={label} className="min-w-0 rounded-md bg-bg-1 px-2.5 py-2">
      <div className="flex min-w-0 items-center gap-2">
        <span className="shrink-0 font-sans text-xs font-strong text-tx-2">{label}</span>
        {tree ? (
          <>
            <span aria-hidden="true" className={cn('h-2 w-2 shrink-0 rounded-full', OUTCOME_FILL[requestOutcome(tree.errorCount > 0, status)])} />
            <span className="flex min-w-0 flex-1 items-center gap-1.5 font-sans text-xs">
              <span className="min-w-0 max-w-[40%] shrink-[2] truncate font-strong text-tx-1">{tree.root.service}</span>
              <span aria-hidden="true" className="shrink-0 text-tx-4">›</span>
              <TraceOperationName operation={tree.root.span.operation} className="min-w-0 font-strong text-tx-0" />
              {status !== null && <span className={cn('shrink-0 font-strong tabular-nums', HTTP_STATUS_TEXT[httpStatusTone(status)])}>{status}</span>}
            </span>
          </>
        ) : (
          <span className="min-w-0 flex-1 truncate font-sans text-xs text-tx-2">{sideState(query, t)}</span>
        )}
        <Link
          to={tracePageHref(trace)}
          className="inline-flex shrink-0 items-center gap-1 rounded font-sans text-xs text-[var(--blue-ink)] hover:underline focus-visible:underline focus-visible:outline-none"
          data-testid={`trace-compare-open-${label.toLowerCase()}`}
        >
          {t('compare.open')}
          <ExternalLink aria-hidden="true" className="h-3 w-3" />
        </Link>
      </div>
      <div className="mt-1 flex min-w-0 flex-wrap items-center gap-x-2.5 gap-y-0.5 font-sans text-xs text-tx-2">
        {tree && (
          <>
            <span className="font-strong tabular-nums text-tx-0" data-testid={`trace-compare-duration-${label.toLowerCase()}`}>{formatTraceDurationNs(tree.totalNs)}</span>
            {start?.isValid() && <span className="tabular-nums">{start.format('YYYY-MM-DD HH:mm:ss.SSS')}</span>}
            <span>{t('view.summary.spans', { count: tree.nodes.length })} · {t('view.summary.services', { count: tree.services.length })}</span>
            {tree.errorCount > 0 && <span className="font-strong text-[var(--red-ink)]">{t('view.summary.errors', { count: tree.errorCount })}</span>}
          </>
        )}
        <span className="min-w-0 max-w-[260px] truncate tabular-nums text-tx-3" title={trace.traceId}>{trace.traceId}</span>
      </div>
    </section>
  );
}

function sideState(query: UseQueryResult<TraceResponse>, t: TFunction<'traces'>): string {
  if (query.isLoading) return t('compare.loading');
  if (query.error) return toApiError(query.error).status === 404 ? t('detail.not_found_title') : t('detail.load_error_title');
  return t('detail.not_found_title');
}

/** A service drawn in one colour across both traces: the first trace's colours, then the second's new services. */
function colorsOf(a: TraceTree | null, b: TraceTree | null): Map<string, string> {
  const colors = new Map<string, string>();
  for (const service of a?.services ?? []) colors.set(service.name, service.color);
  for (const service of b?.services ?? []) if (!colors.has(service.name)) colors.set(service.name, serviceColor(colors.size));
  return colors;
}

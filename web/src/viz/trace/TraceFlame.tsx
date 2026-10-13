import { Search } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { Span } from '@/api/web';
import { writeClipboardText } from '@/lib/clipboard';
import { toApiError } from '@/lib/http';
import { ProductState } from '@/product/states';
import { CopyIconButton } from '@/shell/CopyIconButton';
import { cn } from '@/shell/lib/cn';
import { buildSignalJumps } from '@/shell/SignalReference';
import { Badge } from '@/shell/ui/badge';
import { Button } from '@/shell/ui/button';
import { Input } from '@/shell/ui/input';
import { useFiltersStore } from '@/stores/useFiltersStore';
import { colorKeyForService } from '@/viz/serviceColors';
import { useThemePalette } from '@/viz/timeseries/themeAdapter';
import { formatTraceDurationNs } from '@/viz/trace/duration';

import { layoutTrace } from './layout';
import { useTrace } from './loader';
import { SpanInspector } from './span-detail/SpanInspector';
import type { LaidOutTrace } from './types';
import { flattenTrace, signalLabelsForTrace, traceContextWindow } from './waterfall/model';
import { WATERFALL_GRID, WaterfallRow } from './waterfall/Row';
import { ShareKey } from './waterfall/ShareKey';

interface TraceFlameProps {
  traceId: string;
  initialSpanId?: string | undefined;
  /** Click on a span: typically push a new investigation frame. */
  onSpanClick?: (span: Span) => void;
}

const TIMELINE_TICKS = [0, 25, 50, 75, 100] as const;
const COMPACT_TIMELINE_TICKS = [0, 50, 100] as const;
const MINIMAL_TIMELINE_TICKS = [0, 100] as const;
const END_ONLY_TIMELINE_TICKS = [100] as const;

/** A jump out of the trace, in its head: blue as text, underlined under the pointer and the keyboard. */
const HEAD_LINK =
  'whitespace-nowrap rounded font-semibold text-[var(--blue-ink)] underline-offset-2 hover:underline focus-visible:underline';

function timelineTicksForWidth(width: number): readonly number[] {
  if (width < 120) return END_ONLY_TIMELINE_TICKS;
  if (width < 240) return MINIMAL_TIMELINE_TICKS;
  if (width < 480) return COMPACT_TIMELINE_TICKS;
  return TIMELINE_TICKS;
}

export function TraceFlame({ traceId, initialSpanId, onSpanClick }: TraceFlameProps) {
  const { t } = useTranslation('traces');
  const { data, isLoading, error } = useTrace(traceId);
  const layoutResult = React.useMemo(() => (data ? layoutTrace(data, 'waterfall') : null), [data]);

  if (isLoading) {
    return <ProductState variant="loading" />;
  }
  if (error) {
    // logs 与 traces 分库，按 trace_id 查不到时后端返回 404——属于「未找到」而非故障，
    // 渲染成友好的 empty 态；其余错误才按 error 透出后端 message。
    const notFound = toApiError(error).status === 404;
    return notFound ? (
      <ProductState
        variant="empty"
        title={t('detail.not_found_title')}
        description={t('detail.not_found_description')}
      />
    ) : (
      <ProductState variant="error" title={t('detail.load_error_title')} error={error} />
    );
  }
  if (!layoutResult) return null;
  if (!layoutResult.ok) {
    const e = layoutResult.error;
    const description =
      e.kind === 'multiple_roots'
        ? t('detail.malformed_multiple_roots', { count: e.rootCount })
        : e.kind === 'no_root'
          ? t('detail.malformed_no_root')
          : t('detail.malformed_empty');
    return <ProductState variant="error" title={t('detail.malformed_title')} description={description} />;
  }

  return <JaegerTraceView layout={layoutResult.data} initialSpanId={initialSpanId} onSpanClick={onSpanClick} />;
}

function JaegerTraceView({
  layout,
  initialSpanId,
  onSpanClick,
}: {
  layout: LaidOutTrace;
  initialSpanId?: string | undefined;
  onSpanClick?: ((span: Span) => void) | undefined;
}) {
  const { t } = useTranslation('traces');
  const { palette } = useThemePalette();
  const globalFilters = useFiltersStore((state) => state.filters);
  const [query, setQuery] = React.useState('');
  const requestedInitialSpanId = initialSpanId?.trim() ?? '';
  const initialSpanExists = React.useMemo(
    () => Boolean(requestedInitialSpanId && layout.nodes.some((node) => node.span.span_id === requestedInitialSpanId)),
    [layout.nodes, requestedInitialSpanId],
  );
  const resolvedInitialSpanId = initialSpanExists ? requestedInitialSpanId : layout.trace.root_span_id;
  const [selectedSpanId, setSelectedSpanId] = React.useState(resolvedInitialSpanId);
  const [detailOpen, setDetailOpen] = React.useState(initialSpanExists);
  const [collapsed, setCollapsed] = React.useState<Set<string>>(new Set());
  const timelineHeaderRef = React.useRef<HTMLDivElement>(null);
  const [timelineWidth, setTimelineWidth] = React.useState(600);

  React.useEffect(() => {
    setSelectedSpanId(resolvedInitialSpanId);
    setDetailOpen(initialSpanExists);
    setCollapsed(new Set());
  }, [initialSpanExists, layout.trace.trace_id, resolvedInitialSpanId]);

  React.useEffect(() => {
    const element = timelineHeaderRef.current;
    if (!element) return undefined;

    const measure = () => setTimelineWidth(element.getBoundingClientRect().width);
    measure();
    if (typeof ResizeObserver === 'undefined') return undefined;

    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  const rows = React.useMemo(() => flattenTrace(layout, collapsed), [layout, collapsed]);
  const timelineTicks = timelineTicksForWidth(timelineWidth);
  const serviceCount = React.useMemo(
    () => new Set(layout.nodes.map((node) => node.span.service)).size,
    [layout.nodes],
  );
  const errorCount = layout.nodes.filter((node) => node.span.status === 'ERROR').length;
  const selectedNode =
    layout.nodes.find((node) => node.span.span_id === selectedSpanId) ?? layout.nodes[0] ?? null;
  const traceStartNs = selectedNode
    ? selectedNode.span.start_ns - selectedNode.startOffsetNs
    : layout.trace.spans[0]?.start_ns ?? 0;
  const totalNs = Math.max(1, layout.totalDurationNs);
  const traceTime = React.useMemo(
    () => traceContextWindow(traceStartNs, totalNs),
    [totalNs, traceStartNs],
  );
  const rootSpan =
    layout.nodes.find((node) => node.span.span_id === layout.trace.root_span_id)?.span
    ?? layout.nodes[0]?.span;
  const traceLabels = rootSpan
    ? signalLabelsForTrace(layout.trace.trace_id, rootSpan)
    : { trace_id: layout.trace.trace_id };
  const traceJumps = buildSignalJumps(
    'trace_id',
    layout.trace.trace_id,
    traceTime,
    {
      labels: traceLabels,
      source: { type: 'trace', id: layout.trace.trace_id },
    },
    globalFilters,
  );
  const relatedLogsJump = traceJumps.find((jump) => jump.id === 'logs');
  const serviceMetricsJump = traceJumps.find((jump) => jump.id === 'metrics');
  const needle = query.trim().toLowerCase();
  const matches = React.useMemo(() => {
    if (!needle) return new Set<string>();
    const found = new Set<string>();
    for (const node of layout.nodes) {
      const text = `${node.span.service} ${node.span.operation} ${node.span.span_id} ${node.span.status}`.toLowerCase();
      if (text.includes(needle)) found.add(node.span.span_id);
    }
    return found;
  }, [layout.nodes, needle]);

  const toggle = (spanId: string) => {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(spanId)) next.delete(spanId);
      else next.add(spanId);
      return next;
    });
  };

  const selectSpan = (span: Span) => {
    setSelectedSpanId(span.span_id);
    setDetailOpen(true);
    onSpanClick?.(span);
  };

  const collapseAll = () => {
    setCollapsed(new Set(layout.nodes.filter((node) => node.childIds.length > 0).map((node) => node.span.span_id)));
  };

  return (
    <div className="flex min-h-[680px] w-full max-w-full flex-col overflow-hidden rounded-md border border-bd-0 bg-bg-1">
      <div className="flex flex-wrap items-center gap-3 border-b border-bd-0 px-4 py-3">
        <div className="min-w-[280px] flex-[1_1_360px]">
          <div className="flex min-w-0 items-center gap-1.5">
            <span className="shrink-0 font-sans text-xs font-strong text-tx-2">{t('detail.trace_label')}</span>
            <code
              className="min-w-0 truncate font-mono text-xs font-semibold text-tx-0"
              data-testid="trace-id"
              title={layout.trace.trace_id}
            >
              {layout.trace.trace_id}
            </code>
            <CopyIconButton
              className="h-6 w-6 shrink-0 text-tx-3 hover:text-tx-0"
              label={t('detail.copy_trace_id')}
              onClick={() => {
                void writeClipboardText(layout.trace.trace_id);
              }}
            />
            {layout.trace.truncated && <Badge variant="destructive">{t('detail.truncated')}</Badge>}
          </div>
          <div className="mt-1.5 flex flex-wrap items-center gap-x-2 gap-y-1 font-sans text-xs text-tx-2">
            <span
              className="whitespace-nowrap"
              data-testid="trace-total-duration"
            >
              <span className="font-mono text-sm font-semibold tabular-nums text-tx-0">
                {formatTraceDurationNs(totalNs)}
              </span>{' '}
              <span className="text-tx-3">{t('detail.total_duration')}</span>
            </span>
            <span aria-hidden="true" className="text-tx-3">·</span>
            <span className="whitespace-nowrap">{t('detail.span_count', { count: layout.nodes.length })}</span>
            <span aria-hidden="true" className="text-tx-3">·</span>
            <span className="whitespace-nowrap">{t('detail.service_count', { count: serviceCount })}</span>
            <span aria-hidden="true" className="text-tx-3">·</span>
            <span className={cn('whitespace-nowrap', errorCount > 0 ? 'text-red' : 'text-tx-2')}>
              {t('detail.error_count', { count: errorCount })}
            </span>
            {relatedLogsJump && (
              <>
                <span aria-hidden="true" className="text-tx-3">·</span>
                <Link to={relatedLogsJump.to} className={HEAD_LINK}>
                  {t('detail.related_logs')}
                </Link>
              </>
            )}
            {serviceMetricsJump && (
              <>
                <span aria-hidden="true" className="text-tx-3">·</span>
                <Link to={serviceMetricsJump.to} className={HEAD_LINK}>
                  {t('detail.service_metrics')}
                </Link>
              </>
            )}
          </div>
        </div>
        <div className="ml-auto flex min-w-[280px] flex-[1_1_420px] items-center justify-end gap-2">
          <div className="relative w-full max-w-[360px]">
            <Search className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-tx-3" />
            <Input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              aria-label={t('detail.search_spans')}
              placeholder={t('detail.search_spans')}
              className="h-8 pl-8 font-sans text-xs"
            />
          </div>
          <Button variant="outline" size="sm" onClick={() => setCollapsed(new Set())}>
            {t('detail.expand_all')}
          </Button>
          <Button variant="outline" size="sm" onClick={collapseAll}>
            {t('detail.collapse_all')}
          </Button>
        </div>
      </div>

      <div className="flex min-h-0 min-w-0 flex-1 overflow-hidden">
        <div
          className="min-w-0 flex-1 overflow-x-hidden overflow-y-auto"
          data-testid="trace-waterfall-viewport"
        >
          <div className="w-full min-w-0">
            <div
              className={cn(
                'sticky top-0 z-10 grid border-b border-bd-0 bg-bg-1',
                WATERFALL_GRID,
              )}
            >
              <div className="flex min-w-0 items-center justify-between gap-3 border-r border-bd-0 px-4 py-2 font-sans text-xs font-strong tracking-normal text-tx-2">
                <span className="truncate">{t('detail.column_span')}</span>
                {/* Beside the timeline it explains; where the column is narrow the names keep the room. */}
                <ShareKey className="hidden xl:flex" />
              </div>
              <div className="relative min-w-0 px-4 py-2">
                <div ref={timelineHeaderRef} className="relative mr-[72px] h-5">
                  {timelineTicks.map((tick) => (
                    <div
                      key={tick}
                      className="absolute top-0 whitespace-nowrap font-mono text-xs text-tx-3"
                      style={{
                        left: `${tick}%`,
                        transform:
                          tick === 0
                            ? 'translateX(0)'
                            : tick === 100
                              ? 'translateX(-100%)'
                              : 'translateX(-50%)',
                      }}
                    >
                      {tick === 0 ? '0' : formatTraceDurationNs((totalNs * tick) / 100)}
                    </div>
                  ))}
                </div>
              </div>
            </div>

            {rows.map(({ node, children }) => {
              const serviceColorKey = colorKeyForService(node.span.service);

              return (
                <WaterfallRow
                  key={node.span.span_id}
                  traceId={layout.trace.trace_id}
                  node={node}
                  hasChildren={children.length > 0}
                  collapsed={collapsed.has(node.span.span_id)}
                  selected={selectedSpanId === node.span.span_id}
                  matched={matches.has(node.span.span_id)}
                  // Red is a failed span's; a service that would be drawn in it takes purple.
                  serviceColor={palette[serviceColorKey === '--red' ? '--purple' : serviceColorKey]}
                  totalNs={totalNs}
                  traceStartNs={traceStartNs}
                  time={traceTime}
                  ticks={timelineTicks}
                  onSelect={selectSpan}
                  onToggle={toggle}
                />
              );
            })}
          </div>
        </div>
        {selectedNode && detailOpen && (
          <SpanInspector
            traceId={layout.trace.trace_id}
            span={selectedNode.span}
            durationNs={selectedNode.durationNs}
            startOffsetNs={selectedNode.startOffsetNs}
            totalNs={totalNs}
            time={traceTime}
            onClose={() => setDetailOpen(false)}
          />
        )}
      </div>
    </div>
  );
}

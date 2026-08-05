import {
  ChevronDown,
  ChevronRight,
  Circle,
  GitBranch,
  Search,
  X,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { Span } from '@/api/web';
import { toApiError } from '@/lib/http';
import { ProductState } from '@/product/states';
import { CopyIconButton } from '@/shell/CopyIconButton';
import { cn } from '@/shell/lib/cn';
import {
  buildSignalJumps,
  SignalReference,
  type SignalReferenceTime,
} from '@/shell/SignalReference';
import { Badge } from '@/shell/ui/badge';
import { Button } from '@/shell/ui/button';
import { Input } from '@/shell/ui/input';
import { useFiltersStore } from '@/stores/useFiltersStore';
import { useThemePalette } from '@/viz/timeseries/themeAdapter';
import { colorKeyForService } from '@/viz/trace/colors';
import { formatTraceDurationNs } from '@/viz/trace/duration';
import { TraceOperationName } from '@/viz/trace/TraceOperationName';

import { layoutTrace } from './layout';
import { useTrace } from './loader';
import type { LaidOutTrace, SpanNode } from './types';

interface TraceFlameProps {
  traceId: string;
  initialSpanId?: string | undefined;
  /** Click on a span: typically push a new investigation frame. */
  onSpanClick?: (span: Span) => void;
}

interface TraceRow {
  node: SpanNode;
  children: SpanNode[];
}

const TIMELINE_TICKS = [0, 25, 50, 75, 100] as const;
const COMPACT_TIMELINE_TICKS = [0, 50, 100] as const;
const MINIMAL_TIMELINE_TICKS = [0, 100] as const;
const END_ONLY_TIMELINE_TICKS = [100] as const;
const TRACE_WATERFALL_GRID =
  'grid-cols-[minmax(260px,40%)_minmax(0,1fr)] xl:grid-cols-[minmax(360px,420px)_minmax(0,1fr)]';

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

  const collapseAll = () => {
    setCollapsed(new Set(layout.nodes.filter((node) => node.childIds.length > 0).map((node) => node.span.span_id)));
  };

  return (
    <div className="flex min-h-[680px] w-full max-w-full flex-col overflow-hidden rounded-md border border-bd-0 bg-bg-1">
      <div className="flex flex-wrap items-center gap-3 border-b border-bd-0 px-4 py-3">
        <div className="min-w-[280px] flex-[1_1_360px]">
          <div className="flex min-w-0 items-center gap-1.5">
            <span className="shrink-0 font-sans text-xs font-strong text-tx-2">Trace</span>
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
                void navigator.clipboard?.writeText(layout.trace.trace_id);
              }}
            />
            {layout.trace.truncated && <Badge variant="destructive">truncated</Badge>}
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
                <Link
                  to={relatedLogsJump.to}
                  className="whitespace-nowrap font-semibold text-blue-soft underline-offset-2 hover:text-blue hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue/40"
                >
                  {t('detail.related_logs')}
                </Link>
              </>
            )}
            {serviceMetricsJump && (
              <>
                <span aria-hidden="true" className="text-tx-3">·</span>
                <Link
                  to={serviceMetricsJump.to}
                  className="whitespace-nowrap font-semibold text-blue-soft underline-offset-2 hover:text-blue hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-blue/40"
                >
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
              placeholder="Search service, operation, span id..."
              className="h-8 pl-8 font-sans text-xs"
            />
          </div>
          <Button variant="outline" size="sm" onClick={() => setCollapsed(new Set())}>
            Expand
          </Button>
          <Button variant="outline" size="sm" onClick={collapseAll}>
            Collapse
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
                TRACE_WATERFALL_GRID,
              )}
            >
              <div className="border-r border-bd-0 px-4 py-2 font-sans text-xs font-strong uppercase tracking-normal text-tx-2">
                Service / operation
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
              const selected = selectedSpanId === node.span.span_id;
              const match = matches.has(node.span.span_id);
              const left = pct(node.startOffsetNs, totalNs);
              const width = Math.max(0.35, pct(node.durationNs, totalNs));
              const end = Math.min(100, left + width);
              const durationLabel = formatTraceDurationNs(node.durationNs);
              const serviceColorKey = colorKeyForService(node.span.service);
              const color = palette[serviceColorKey === '--red' ? '--purple' : serviceColorKey];
              const signalLabels = signalLabelsForSpan(layout.trace.trace_id, node.span);
              const source = { type: 'trace' as const, id: layout.trace.trace_id };
              return (
                <div
                  key={node.span.span_id}
                  role="button"
                  tabIndex={0}
                  onClick={() => {
                    setSelectedSpanId(node.span.span_id);
                    setDetailOpen(true);
                    onSpanClick?.(node.span);
                  }}
                  onKeyDown={(event) => {
                    if (event.key === 'Enter' || event.key === ' ') {
                      event.preventDefault();
                      setSelectedSpanId(node.span.span_id);
                      setDetailOpen(true);
                      onSpanClick?.(node.span);
                    }
                  }}
                  className={cn(
                    'grid min-h-[42px] w-full cursor-pointer border-b border-bd-0 text-left transition-colors hover:bg-bg-2',
                    TRACE_WATERFALL_GRID,
                    selected && 'bg-bg-2',
                  )}
                >
                  <div className="flex min-w-0 items-center border-r border-bd-0 px-3 py-1.5">
                    <span style={{ width: node.depth * 16 }} className="shrink-0" />
                    <span
                      role="button"
                      tabIndex={0}
                      onClick={(event) => {
                        if (children.length > 0) {
                          event.stopPropagation();
                          toggle(node.span.span_id);
                          return;
                        }
                        setSelectedSpanId(node.span.span_id);
                        setDetailOpen(true);
                        onSpanClick?.(node.span);
                      }}
                      onKeyDown={(event) => {
                        if (event.key !== 'Enter' && event.key !== ' ') return;
                        event.preventDefault();
                        if (children.length > 0) {
                          event.stopPropagation();
                          toggle(node.span.span_id);
                          return;
                        }
                        setSelectedSpanId(node.span.span_id);
                        setDetailOpen(true);
                        onSpanClick?.(node.span);
                      }}
                      className="mr-1 grid h-5 w-5 shrink-0 place-items-center rounded text-tx-3 hover:bg-bg-3 hover:text-tx-0"
                      aria-label={children.length > 0 ? 'Toggle span children' : 'Leaf span'}
                    >
                      {children.length > 0 ? (
                        collapsed.has(node.span.span_id) ? (
                          <ChevronRight className="h-3.5 w-3.5" />
                        ) : (
                          <ChevronDown className="h-3.5 w-3.5" />
                        )
                      ) : (
                        <Circle className="h-2 w-2 fill-current" />
                      )}
                    </span>
                    <span
                      className="mr-2 h-2.5 w-2.5 shrink-0 rounded-sm"
                      style={{ backgroundColor: color }}
                    />
                    <div className="min-w-0 flex-1">
                      <div className="flex min-w-0 items-center">
                        <SignalReference
                          type="span_id"
                          value={node.span.span_id}
                          labelName="span_id"
                          labels={signalLabels}
                          time={traceTime}
                          source={source}
                          showIcon={false}
                          className="min-w-0 flex-1 justify-start text-left text-tx-0 decoration-current/30 hover:text-tx-0 [&>span]:min-w-0 [&>span]:flex-1"
                        >
                          <TraceOperationName
                            operation={node.span.operation}
                            className="min-w-0 w-full font-sans text-xs font-strong text-tx-0"
                          />
                        </SignalReference>
                      </div>
                      <div className="mt-0.5 flex min-w-0 items-center gap-2 font-sans text-xs text-tx-3">
                        <SignalReference
                          type="service"
                          value={node.span.service}
                          labelName="service.name"
                          labels={signalLabels}
                          time={traceTime}
                          source={source}
                          showIcon={false}
                          className="min-w-0 max-w-full truncate text-tx-3 decoration-current/30 hover:text-indigo-soft [&>span]:truncate"
                        >
                          {node.span.service}
                        </SignalReference>
                        {node.span.status === 'ERROR' && <span className="text-red">error</span>}
                        {match && <span className="text-blue-soft">match</span>}
                      </div>
                    </div>
                  </div>
                  <div className="relative min-w-0 px-4 py-2">
                    <div className="relative h-full min-h-6 w-full">
                      <div
                        className="absolute inset-y-0 left-0 right-[72px]"
                        data-testid="trace-timeline-track"
                      >
                        {timelineTicks.map((tick) => (
                          <span
                            key={tick}
                            className="absolute top-0 h-full w-px bg-bd-0"
                            style={{ left: `${tick}%` }}
                          />
                        ))}
                        <div
                          role="img"
                          aria-label={`${node.span.operation}, ${node.span.service}, ${t('explore.table.duration')} ${durationLabel}`}
                          className={cn(
                            // Span outline uses the overlay-soft token so it
                            // tracks the theme palette instead of forcing a
                            // dark-ish edge on a light canvas.
                            'absolute top-1/2 h-5 -translate-y-1/2 cursor-pointer rounded-sm border border-overlay-soft transition-[filter] hover:brightness-110',
                            node.span.status === 'ERROR' && 'ring-1 ring-red',
                            match && 'ring-2 ring-blue-soft',
                          )}
                          data-testid="trace-span-bar"
                          style={{ left: `${left}%`, width: `${width}%`, backgroundColor: color }}
                        />
                        <span
                          aria-label={`${t('explore.table.duration')}: ${durationLabel}`}
                          className="pointer-events-none absolute top-1/2 z-[1] ml-1.5 -translate-y-1/2 whitespace-nowrap font-mono text-xs font-medium tabular-nums text-tx-1"
                          data-testid="trace-span-duration"
                          style={{ left: `${end}%` }}
                        >
                          {durationLabel}
                        </span>
                        {node.span.events.map((event, index) => (
                          <span
                            key={`${node.span.span_id}-${event.name}-${index}`}
                            className="absolute top-1/2 h-6 w-px -translate-y-1/2 bg-tx-0"
                            title={event.name}
                            style={{ left: `${pct(event.ts_ns - traceStartNs, totalNs)}%` }}
                          />
                        ))}
                      </div>
                    </div>
                  </div>
                </div>
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

function SpanInspector({
  traceId,
  span,
  durationNs,
  startOffsetNs,
  totalNs,
  time,
  onClose,
}: {
  traceId: string;
  span: Span;
  durationNs: number;
  startOffsetNs: number;
  totalNs: number;
  time?: SignalReferenceTime | undefined;
  onClose: () => void;
}) {
  const { t } = useTranslation('traces');
  const labels = signalLabelsForSpan(traceId, span);
  const source = { type: 'trace' as const, id: traceId };
  return (
    <aside className="flex w-[380px] shrink-0 flex-col border-l border-bd-0 bg-bg-0">
      <div className="flex items-start gap-3 border-b border-bd-0 px-4 py-3">
        <div className="min-w-0 flex-1">
          <SignalReference
            type="span_id"
            value={span.span_id}
            labelName="span_id"
            labels={labels}
            time={time}
            source={source}
            showIcon={false}
            className="max-w-full justify-start text-left text-tx-0 decoration-current/30"
          >
            <TraceOperationName
              operation={span.operation}
              className="font-sans text-sm font-strong text-tx-0"
            />
          </SignalReference>
          <div className="mt-0.5 flex flex-wrap gap-3 font-sans text-xs text-tx-2">
            <SignalReference
              type="service"
              value={span.service}
              labelName="service.name"
              labels={labels}
              time={time}
              source={source}
              showIcon={false}
              className="text-tx-2 decoration-current/30 hover:text-indigo-soft"
            >
              {span.service}
            </SignalReference>
            <span>{formatTraceDurationNs(durationNs)}</span>
            <span>start {formatTraceDurationNs(startOffsetNs)}</span>
            <span>{pct(startOffsetNs, totalNs).toFixed(1)}% into trace</span>
          </div>
        </div>
        <Button variant="ghost" size="icon" className="h-7 w-7 shrink-0" onClick={onClose} aria-label="Close span details">
          <X className="h-3.5 w-3.5" />
        </Button>
      </div>
      <div className="min-h-0 flex-1 overflow-auto">
        <div className="border-b border-bd-0 px-4 py-3">
          <div className="mb-2 font-sans text-xs font-strong uppercase tracking-normal text-tx-2">
            Span id
          </div>
          <div className="flex items-center gap-2">
            <code className="min-w-0 flex-1 truncate rounded border border-bd-0 bg-bg-1 px-2 py-1 font-mono text-xs text-tx-1">
              {span.span_id}
            </code>
            <CopyIconButton
              label={t('detail.copy_span_id')}
              onClick={() => {
                void navigator.clipboard?.writeText(span.span_id);
              }}
            />
          </div>
        </div>
        <div className="grid grid-cols-3 border-b border-bd-0">
          <DetailMetric label="duration" value={formatTraceDurationNs(durationNs)} />
          <DetailMetric label="start" value={formatTraceDurationNs(startOffsetNs)} />
          <DetailMetric label="offset" value={`${pct(startOffsetNs, totalNs).toFixed(1)}%`} />
        </div>
        <div className="min-w-0 border-b border-bd-0 p-4">
          <div className="mb-2 flex items-center gap-2 font-sans text-xs font-strong uppercase tracking-normal text-tx-2">
            <GitBranch className="h-3.5 w-3.5" /> Tags
          </div>
          <pre className="max-h-[280px] overflow-auto rounded-md border border-bd-0 bg-bg-1 p-3 font-mono text-xs leading-5 text-tx-1">
            {formatJson(span.attributes)}
          </pre>
        </div>
        <div className="min-w-0 p-4">
          <div className="mb-2 font-sans text-xs font-strong uppercase tracking-normal text-tx-2">
            Events
          </div>
          {span.events.length === 0 ? (
            <div className="rounded-md border border-dashed border-bd-1 bg-bg-1 p-3 font-sans text-xs text-tx-3">
              No span events.
            </div>
          ) : (
            <div className="space-y-2">
              {span.events.map((event, index) => (
                <div key={`${event.name}-${index}`} className="rounded-md border border-bd-0 bg-bg-1 p-2">
                  <div className="font-sans text-xs font-strong text-tx-0">{event.name}</div>
                  <pre className="mt-1 max-h-[120px] overflow-auto font-mono text-xs leading-4 text-tx-2">
                    {formatJson(event.attributes)}
                  </pre>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
    </aside>
  );
}

function DetailMetric({ label, value }: { label: string; value: string }) {
  return (
    <div className="min-w-0 border-r border-bd-0 px-4 py-3 last:border-r-0">
      <div className="font-sans text-xs font-semibold uppercase tracking-normal text-tx-3">{label}</div>
      <div className="mt-1 truncate font-sans text-sm font-semibold text-tx-0">{value}</div>
    </div>
  );
}

const TRACE_CONTEXT_PADDING_MS = 5 * 60 * 1000;

function traceContextWindow(traceStartNs: number, totalNs: number): SignalReferenceTime | undefined {
  const startMs = traceStartNs / 1_000_000;
  const endMs = (traceStartNs + totalNs) / 1_000_000;
  if (!Number.isFinite(startMs) || !Number.isFinite(endMs) || startMs <= 0 || endMs < startMs) {
    return undefined;
  }
  const from = new Date(startMs - TRACE_CONTEXT_PADDING_MS);
  const to = new Date(endMs + TRACE_CONTEXT_PADDING_MS);
  if (Number.isNaN(from.getTime()) || Number.isNaN(to.getTime())) return undefined;
  return { from: from.toISOString(), to: to.toISOString() };
}

function signalLabelsForTrace(traceId: string, span: Span): Record<string, string> {
  return {
    ...stringSignalAttributes(span.attributes),
    trace_id: traceId,
    service_name: span.service,
    operation_name: span.operation,
  };
}

function signalLabelsForSpan(traceId: string, span: Span): Record<string, string> {
  return {
    ...signalLabelsForTrace(traceId, span),
    span_id: span.span_id,
  };
}

function stringSignalAttributes(
  attributes: Record<string, unknown>,
  prefix = '',
  depth = 0,
): Record<string, string> {
  const labels: Record<string, string> = {};
  for (const [key, value] of Object.entries(attributes)) {
    const qualifiedKey = prefix ? `${prefix}.${key}` : key;
    if (typeof value === 'string' || typeof value === 'number' || typeof value === 'boolean') {
      labels[qualifiedKey] = String(value);
      continue;
    }
    if (depth < 1 && value && typeof value === 'object' && !Array.isArray(value)) {
      Object.assign(
        labels,
        stringSignalAttributes(value as Record<string, unknown>, qualifiedKey, depth + 1),
      );
    }
  }
  return labels;
}

function flattenTrace(layout: LaidOutTrace, collapsed: Set<string>): TraceRow[] {
  const byId = new Map(layout.nodes.map((node) => [node.span.span_id, node]));
  const children = new Map<string, SpanNode[]>();
  const roots: SpanNode[] = [];

  for (const node of layout.nodes) {
    const parentId = node.span.parent_span_id;
    if (parentId && byId.has(parentId)) {
      const list = children.get(parentId) ?? [];
      list.push(node);
      children.set(parentId, list);
    } else {
      roots.push(node);
    }
  }
  for (const list of children.values()) {
    list.sort((a, b) => a.startOffsetNs - b.startOffsetNs || a.span.operation.localeCompare(b.span.operation));
  }
  roots.sort((a, b) => a.startOffsetNs - b.startOffsetNs);

  const rows: TraceRow[] = [];
  const walk = (node: SpanNode) => {
    const childRows = children.get(node.span.span_id) ?? [];
    rows.push({ node, children: childRows });
    if (collapsed.has(node.span.span_id)) return;
    for (const child of childRows) walk(child);
  };
  for (const root of roots) walk(root);
  return rows;
}

function pct(value: number, total: number): number {
  if (!Number.isFinite(value) || !Number.isFinite(total) || total <= 0) return 0;
  return Math.max(0, Math.min(100, (value / total) * 100));
}

function formatJson(value: unknown): string {
  if (typeof value === 'string') {
    try {
      return JSON.stringify(JSON.parse(value), null, 2);
    } catch {
      return value;
    }
  }
  return JSON.stringify(value ?? {}, null, 2);
}

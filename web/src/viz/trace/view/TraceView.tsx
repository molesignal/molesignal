import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';

import type { Span, TraceWindow } from '@/api/web';
import { toApiError } from '@/lib/http';
import { ProductState } from '@/product/states';
import { cn } from '@/shell/lib/cn';
import { ResizeHandle } from '@/shell/ResizeHandle';
import { buildSignalJumps } from '@/shell/SignalReference';
import { type GlobalFilter, useFiltersStore } from '@/stores/useFiltersStore';

import { CompareWith } from '../compare/CompareWith';
import { FlameGraph } from '../flame/FlameGraph';
import { useTrace } from '../loader';
import { TraceExportMenu } from './Export';
import { TRACE_LAYOUT_LIMITS, useTraceLayout } from './layout';
import { rumSessionHref, useTraceSession } from './rumSession';
import { useTraceViewState } from './state';
import { type TraceJump, TraceSummary } from './Summary';
import { TraceLegendRow, TraceToolbar } from './Toolbar';
import { TraceMap } from '../map/TraceMap';
import { buildTraceTree, type TraceTree } from '../model/tree';
import { LINK_SEARCH_PADDING_MICROS, type OpenLinkedTrace } from '../span-detail/Links';
import { profilesHref, profileWindow, useTraceProfiles } from '../span-detail/profileLinks';
import { SpanPanel, type SpanPanelTab } from '../span-detail/SpanPanel';
import { SpanGroups } from '../spanList/SpanGroups';
import { signalLabelsForSpan, signalLabelsForTrace, traceContextWindow } from '../waterfall/model';
import { Waterfall } from '../waterfall/Waterfall';

/** The room the waterfall keeps beside a docked span panel; with less, the panel floats over it. */
const VIEW_ROOM = 520;
/** How far around the trace its logs are read. */
const LOG_PADDING_MICROS = 5 * 60 * 1_000_000;

export interface TraceViewProps {
  traceId: string;
  /** When the trace's spans began, where the place that opens it knows. */
  window?: TraceWindow | undefined;
  initialSpanId?: string | null | undefined;
  /** The place it stands in: a drawer over a list, its own page, an investigation frame. */
  frame: 'drawer' | 'page' | 'frame';
  /** The place's own controls, at the end of the head. */
  actions?: React.ReactNode;
  /** The selected span changed. */
  onSpanSelect?: ((span: Span | null) => void) | undefined;
  /** A link was followed into another trace. By default its page opens. */
  onOpenTrace?: OpenLinkedTrace | undefined;
  className?: string | undefined;
}

/**
 * One trace in full: its head fixed at the top, a waterfall, flame graph,
 * span list or service map under it scrolling on its own, and the detail of
 * the selected span in a panel beside it whose width the reader sets.
 */
export function TraceView({ traceId, window, initialSpanId, frame, actions, onSpanSelect, onOpenTrace, className }: TraceViewProps) {
  const { t } = useTranslation('traces');
  const { data, isLoading, error } = useTrace(traceId, window);
  const tree = React.useMemo(() => (data ? buildTraceTree(data) : null), [data]);

  if (isLoading) return <ProductState variant="loading" />;

  if (error) {
    // Logs and traces are stored apart: a trace id with no spans in the window is not found, not a failure.
    return toApiError(error).status === 404 ? (
      <ProductState variant="empty" title={t('detail.not_found_title')} description={t('detail.not_found_description')} />
    ) : (
      <ProductState variant="error" title={t('detail.load_error_title')} error={error} />
    );
  }

  if (!tree) return <ProductState variant="empty" title={t('detail.not_found_title')} description={t('detail.malformed_empty')} />;

  return (
    <LoadedTrace
      tree={tree}
      initialSpanId={initialSpanId}
      frame={frame}
      actions={actions}
      onSpanSelect={onSpanSelect}
      onOpenTrace={onOpenTrace}
      className={className}
    />
  );
}

function LoadedTrace({
  tree,
  initialSpanId,
  frame,
  actions,
  onSpanSelect,
  onOpenTrace,
  className,
}: Omit<TraceViewProps, 'traceId' | 'window'> & { tree: TraceTree }) {
  const { t } = useTranslation('traces');
  const navigate = useNavigate();
  const filters = useFiltersStore((store) => store.filters);
  const state = useTraceViewState(tree, initialSpanId);
  const layout = useTraceLayout();
  const bodyRef = React.useRef<HTMLDivElement>(null);
  const [bodyWidth, setBodyWidth] = React.useState(0);
  const [panelOpen, setPanelOpen] = React.useState(true);
  const [tab, setTab] = React.useState<SpanPanelTab>('attributes');

  React.useEffect(() => {
    const element = bodyRef.current;
    if (!element) return undefined;
    const measure = () => setBodyWidth(element.getBoundingClientRect().width);
    measure();
    if (typeof ResizeObserver === 'undefined') return undefined;
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  const colors = React.useMemo(() => new Map(tree.services.map((service) => [service.name, service.color])), [tree]);
  const selected = state.selectedId ? tree.byId.get(state.selectedId) ?? null : null;

  React.useEffect(() => {
    onSpanSelect?.(selected?.span ?? null);
    // Only a change of the selected span is news.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selected?.id]);

  const time = React.useMemo(() => traceContextWindow(tree.startNs, tree.totalNs), [tree]);
  const startMicros = Math.floor(tree.startNs / 1_000);
  const endMicros = Math.ceil(tree.endNs / 1_000);
  const logWindow = React.useMemo(() => ({ from: startMicros - LOG_PADDING_MICROS, to: endMicros + LOG_PADDING_MICROS }), [endMicros, startMicros]);
  const linkWindow = React.useMemo(
    () => ({ from: startMicros - LINK_SEARCH_PADDING_MICROS, to: endMicros + LINK_SEARCH_PADDING_MICROS }),
    [endMicros, startMicros],
  );

  const profiles = useTraceProfiles(tree.traceId, tree.startNs, tree.endNs);
  const linkedProfiles = (profiles.data?.all.length ?? 0) > 0;
  const session = useTraceSession(tree);
  const jumps = React.useMemo(
    () => traceJumps(tree, time, filters, { linkedProfiles, session }),
    [filters, linkedProfiles, session, time, tree],
  );
  const spanLogsHref = React.useMemo(() => {
    if (!selected) return undefined;
    const labels = signalLabelsForSpan(tree.traceId, selected.span);
    return buildSignalJumps('span_id', selected.span.spanId, time, { labels }, filters).find((jump) => jump.id === 'logs')?.to;
  }, [filters, selected, time, tree.traceId]);

  const openTrace: OpenLinkedTrace = onOpenTrace ?? ((traceId, spanId, aroundNs) => {
    const params = new URLSearchParams();
    if (spanId) params.set('spanId', spanId);
    if (aroundNs) {
      params.set('from', String(Math.floor(aroundNs / 1_000) - LINK_SEARCH_PADDING_MICROS));
      params.set('to', String(Math.floor(aroundNs / 1_000) + LINK_SEARCH_PADDING_MICROS));
    }
    navigate(`/traces/${encodeURIComponent(traceId)}${params.size > 0 ? `?${params}` : ''}`);
  });

  const docked = bodyWidth === 0 || bodyWidth >= VIEW_ROOM + TRACE_LAYOUT_LIMITS.panel.min;
  const panelMax = Math.max(TRACE_LAYOUT_LIMITS.panel.min, Math.min(TRACE_LAYOUT_LIMITS.panel.max, (bodyWidth || 1200) - VIEW_ROOM));
  const panelWidth = Math.min(panelMax, layout.widths.panel ?? TRACE_LAYOUT_LIMITS.panel.fallback);
  const namesWidth = layout.widths.names ?? TRACE_LAYOUT_LIMITS.names.fallback;
  const outgoingLinks = React.useMemo(() => tree.nodes.reduce((total, node) => total + node.span.links.length, 0), [tree]);

  const openOperation = (service: string, operation: string) => {
    state.setSearch(`service:"${service}" name:"${operation}"`);
    state.setView('waterfall');
  };

  const view = {
    waterfall: (
      <Waterfall
        tree={tree}
        state={state}
        colors={colors}
        time={time}
        namesWidth={namesWidth}
        profiled={profiles.data?.bySpan}
        onNamesWidthPreview={(names) => layout.preview({ names })}
        onNamesWidthCommit={(names) => layout.commit({ names })}
      />
    ),
    flame: <FlameGraph tree={tree} state={state} colors={colors} />,
    spans: <SpanGroups tree={tree} onOpenOperation={openOperation} />,
    map: (
      <TraceMap
        tree={tree}
        onOpenService={(service) => {
          state.setSearch(`service:"${service}"`);
          state.setView('waterfall');
        }}
      />
    ),
  }[state.view];

  const panel = panelOpen && selected && (
    <SpanPanel
      tree={tree}
      node={selected}
      tab={tab}
      onTabChange={setTab}
      time={time}
      window={logWindow}
      linkWindow={linkWindow}
      logsHref={{ span: spanLogsHref, trace: jumps.find((jump) => jump.key === 'logs')?.to }}
      profiles={profiles.data}
      profilesLoading={profiles.isLoading}
      onSelect={state.select}
      onFocus={state.focus}
      onClose={() => setPanelOpen(false)}
      onOpenTrace={openTrace}
    />
  );

  return (
    <div data-trace-view={frame} className={cn('flex h-full min-h-0 w-full min-w-0 flex-col bg-[var(--functional-surface)]', className)}>
      <TraceSummary
        tree={tree}
        jumps={jumps}
        tools={
          <>
            <CompareWith tree={tree} />
            <TraceExportMenu trace={tree.trace} />
          </>
        }
        actions={actions}
        outgoingLinks={outgoingLinks}
      />
      <TraceToolbar tree={tree} state={state} panelOpen={panelOpen} onOpenPanel={() => setPanelOpen(true)} />
      <div ref={bodyRef} className="relative flex min-h-0 flex-1">
        <div className="flex min-h-0 min-w-0 flex-1 flex-col">
          {(state.view === 'waterfall' || state.view === 'flame') && (
            <TraceLegendRow tree={tree} state={state} colors={colors} namesWidth={namesWidth} />
          )}
          <div className="min-h-0 flex-1">{view}</div>
        </div>
        {panel && docked && (
          <>
            <ResizeHandle
              label={t('view.panel.resize')}
              width={panelWidth}
              min={TRACE_LAYOUT_LIMITS.panel.min}
              max={panelMax}
              grows="left"
              defaultWidth={TRACE_LAYOUT_LIMITS.panel.fallback}
              onWidthChange={(width) => layout.preview({ panel: width })}
              onWidthCommit={(width) => layout.commit({ panel: width })}
              className="-mx-[4px]"
            />
            <div data-span-panel-place="beside" className="min-h-0 shrink-0" style={{ width: panelWidth }}>
              {panel}
            </div>
          </>
        )}
        {panel && !docked && (
          <div data-span-panel-place="over" className="absolute inset-y-0 right-0 z-20 w-[min(380px,90%)] shadow-drawer">{panel}</div>
        )}
      </div>
    </div>
  );
}

/**
 * The ways out of a trace: its logs, its root service's metrics, its profiles
 * and the user session it served. The profiles are those that kept the
 * trace's id where there are any, else every profile of the time it ran.
 */
function traceJumps(
  tree: TraceTree,
  time: ReturnType<typeof traceContextWindow>,
  filters: GlobalFilter[],
  { linkedProfiles, session }: { linkedProfiles: boolean; session: string | undefined },
): TraceJump[] {
  const labels = signalLabelsForTrace(tree.traceId, tree.root.span);
  const signal = buildSignalJumps('trace_id', tree.traceId, time, { labels, source: { type: 'trace', id: tree.traceId } }, filters);
  const jumps: TraceJump[] = [];

  for (const key of ['logs', 'metrics'] as const) {
    const jump = signal.find((candidate) => candidate.id === key);
    if (jump) jumps.push({ key, to: jump.to });
  }

  jumps.push({
    key: 'profiles',
    to: profilesHref(profileWindow(tree.startNs, tree.endNs), linkedProfiles ? { traceId: tree.traceId } : {}),
  });

  if (session) jumps.push({ key: 'session', to: rumSessionHref(session, tree) });

  return jumps;
}

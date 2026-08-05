import {
  ChevronDown,
  ChevronRight,
  Flame,
  GitCompare,
  ListTree,
  Maximize2,
  Minimize2,
  RotateCcw,
  Search,
  Table2,
  ZoomOut,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { DiffFlamebearer, Flamebearer } from '@/api/profiles';
import { cn } from '@/shell/lib/cn';
import { Button } from '@/shell/ui/button';
import { Input } from '@/shell/ui/input';
import { useThemePalette } from '@/viz/timeseries/themeAdapter';

import {
  ancestorAtDepth,
  callTreeChildren,
  decodeNodes,
  diffIntensity,
  diffTone,
  type FlameNode,
  type FlameWindow,
  formatValue,
  heatColor,
  nodeInWindow,
  nodeKey,
  rootTotal,
  topFunctions,
  type TopFunction,
} from './flamebearer';

const ROW_HEIGHT = 24;
const MIN_BAR_PCT = 0.2;
const LABEL_MIN_PCT = 2.5;
const DEFAULT_ANALYSIS_HEIGHT = 360;
const MIN_ANALYSIS_HEIGHT = 320;
const MAX_ANALYSIS_HEIGHT = 640;

export type ProfileView = 'flame' | 'tree' | 'top';

interface FlamegraphProps {
  flamebearer: Flamebearer | DiffFlamebearer;
  diff?: boolean | undefined;
  /** Injected by the page (for example, the number of merged profiles). */
  headerExtra?: React.ReactNode | undefined;
  className?: string | undefined;
  selectedFunction?: string | null | undefined;
  onSelectedFunctionChange?: ((name: string) => void) | undefined;
  service?: string | undefined;
  onCompare?: (() => void) | undefined;
}

interface PointerPosition {
  x: number;
  y: number;
  width: number;
  height: number;
}

export function Flamegraph({
  flamebearer,
  diff = false,
  headerExtra,
  className,
  selectedFunction,
  onSelectedFunctionChange,
  service,
  onCompare,
}: FlamegraphProps) {
  const { t } = useTranslation('profiles');
  const { palette } = useThemePalette();
  const [query, setQuery] = React.useState('');
  const [view, setView] = React.useState<ProfileView>('flame');
  const [focus, setFocus] = React.useState<FlameWindow | null>(null);
  const [hoverNode, setHoverNode] = React.useState<FlameNode | null>(null);
  const [selectedKey, setSelectedKey] = React.useState<string | null>(null);
  const [pointer, setPointer] = React.useState<PointerPosition>({
    x: 0,
    y: 0,
    width: 0,
    height: 0,
  });
  const [analysisHeight, setAnalysisHeight] = React.useState(DEFAULT_ANALYSIS_HEIGHT);
  const [fullscreen, setFullscreen] = React.useState(false);
  const rootRef = React.useRef<HTMLDivElement>(null);
  const containerRef = React.useRef<HTMLDivElement>(null);
  const resizeCleanupRef = React.useRef<(() => void) | null>(null);

  const nodes = React.useMemo(
    () => decodeNodes(flamebearer.levels, diff),
    [flamebearer, diff],
  );
  const byDepth = React.useMemo(() => {
    const grouped: FlameNode[][] = Array.from(
      { length: flamebearer.levels.length },
      () => [],
    );
    for (const node of nodes) grouped[node.depth]?.push(node);
    return grouped;
  }, [nodes, flamebearer.levels.length]);
  const children = React.useMemo(() => callTreeChildren(nodes), [nodes]);
  const functionRows = React.useMemo(
    () => topFunctions(flamebearer, diff),
    [diff, flamebearer],
  );

  React.useEffect(() => {
    setFocus(null);
    setHoverNode(null);
    setSelectedKey(null);
  }, [flamebearer]);

  React.useEffect(() => {
    const handleFullscreenChange = () =>
      setFullscreen(document.fullscreenElement === rootRef.current);
    document.addEventListener('fullscreenchange', handleFullscreenChange);
    return () => document.removeEventListener('fullscreenchange', handleFullscreenChange);
  }, []);

  React.useEffect(() => () => {
    resizeCleanupRef.current?.();
  }, []);

  const names = flamebearer.names;
  const total = rootTotal(flamebearer);
  const maxSelf = flamebearer.maxSelf || 1;
  const maxAbsDelta = 'maxAbsDelta' in flamebearer ? flamebearer.maxAbsDelta : 0;
  const units = flamebearer.units;
  const depthCount = flamebearer.levels.length;
  const needle = query.trim().toLowerCase();
  const defaultFunction = functionRows[0]?.name ?? null;
  const keyedNode = selectedKey
    ? nodes.find((node) => nodeKey(node) === selectedKey) ?? null
    : null;
  const activeFunction =
    selectedFunction ?? (keyedNode ? names[keyedNode.nameIndex] ?? null : null) ?? defaultFunction;
  const selectedNode =
    (keyedNode && names[keyedNode.nameIndex] === activeFunction ? keyedNode : null)
    ?? hottestNodeForName(nodes, names, activeFunction);
  const selectedStats =
    functionRows.find((row) => row.name === activeFunction) ?? null;
  const selectedPath = React.useMemo(
    () => (selectedNode ? callPath(nodes, selectedNode) : []),
    [nodes, selectedNode],
  );
  const selectedPathKeys = React.useMemo(
    () => new Set(selectedPath.map(nodeKey)),
    [selectedPath],
  );
  const selectedChildren = React.useMemo(
    () => (selectedNode
      ? aggregateChildren(children.get(nodeKey(selectedNode)) ?? [], names, selectedNode.total)
      : []),
    [children, names, selectedNode],
  );
  const selectedOccurrences = activeFunction
    ? nodes.filter((node) => names[node.nameIndex] === activeFunction).length
    : 0;

  if (total <= 0 || depthCount === 0) {
    return (
      <div className={cn('rounded-md border border-bd-0 bg-bg-1 p-10 text-center', className)}>
        <div className="font-sans text-sm font-semibold text-tx-0">
          {t('flamegraph.no_data_title')}
        </div>
        <div className="mt-1 font-sans text-xs text-tx-2">
          {t('flamegraph.no_data_description')}
        </div>
      </div>
    );
  }

  const focusWin: FlameWindow = focus ?? { depth: 0, start: 0, total };
  const nameOf = (node: FlameNode) => names[node.nameIndex] ?? '';
  const matches = (node: FlameNode) =>
    needle.length > 0 && nameOf(node).toLowerCase().includes(needle);

  const barStyle = (node: FlameNode): React.CSSProperties => {
    let background: string;
    let opacity = 1;
    if (diff) {
      const tone = diffTone(node.delta);
      const intensity = diffIntensity(node.delta, maxAbsDelta);
      background =
        tone === 'increase'
          ? palette['--red']
          : tone === 'decrease'
            ? palette['--green']
            : palette['--surface-muted'];
      opacity = tone === 'neutral' ? 0.45 : 0.35 + 0.6 * intensity;
    } else {
      // Width encodes cumulative cost; color encodes self cost.
      background = heatColor(node.self / maxSelf);
    }
    if (needle.length > 0 && !matches(node)) opacity *= 0.22;
    return { background, opacity };
  };

  const selectNode = (node: FlameNode, focusNode = false) => {
    const name = nameOf(node);
    setSelectedKey(nodeKey(node));
    onSelectedFunctionChange?.(name);
    if (focusNode) {
      setFocus({ depth: node.depth, start: node.start, total: node.total });
    }
  };

  const selectFunction = (name: string) => {
    const node = hottestNodeForName(nodes, names, name);
    setSelectedKey(node ? nodeKey(node) : null);
    onSelectedFunctionChange?.(name);
  };

  const focusFunction = (name: string) => {
    const node = hottestNodeForName(nodes, names, name);
    if (!node) return;
    setView('flame');
    setQuery('');
    selectNode(node, true);
  };

  const handleMove = (event: React.MouseEvent) => {
    const rect = containerRef.current?.getBoundingClientRect();
    if (!rect) return;
    setPointer({
      x: event.clientX - rect.left,
      y: event.clientY - rect.top,
      width: rect.width,
      height: rect.height,
    });
  };

  const handleResizeStart = (event: React.MouseEvent<HTMLDivElement>) => {
    if (fullscreen) return;
    event.preventDefault();
    resizeCleanupRef.current?.();
    const startY = event.clientY;
    const startHeight = analysisHeight;
    const handleMoveEvent = (moveEvent: MouseEvent) => {
      const next = Math.min(
        MAX_ANALYSIS_HEIGHT,
        Math.max(MIN_ANALYSIS_HEIGHT, startHeight + moveEvent.clientY - startY),
      );
      setAnalysisHeight(next);
    };
    const handleUp = () => {
      window.removeEventListener('mousemove', handleMoveEvent);
      window.removeEventListener('mouseup', handleUp);
      resizeCleanupRef.current = null;
    };
    window.addEventListener('mousemove', handleMoveEvent);
    window.addEventListener('mouseup', handleUp);
    resizeCleanupRef.current = handleUp;
  };

  const toggleFullscreen = () => {
    if (document.fullscreenElement === rootRef.current) {
      void document.exitFullscreen();
      return;
    }
    void rootRef.current?.requestFullscreen();
  };

  const renderBar = (
    node: FlameNode,
    key: string,
    left: number,
    width: number,
    ancestor: boolean,
  ) => {
    const name = nameOf(node);
    const matched = matches(node);
    const selected = selectedNode ? nodeKey(selectedNode) === nodeKey(node) : false;
    return (
      <button
        key={key}
        type="button"
        title={name}
        aria-label={t('flamegraph.frame_aria', { name })}
        onClick={() => selectNode(node, true)}
        onMouseEnter={() => setHoverNode(node)}
        onMouseLeave={() => setHoverNode((current) => (current === node ? null : current))}
        className={cn(
          'absolute flex items-center overflow-hidden rounded-[2px] border border-bg-0 px-1.5 text-left',
          'font-sans text-xs leading-none text-tx-0 transition-[filter] hover:brightness-110',
          'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo',
          matched && 'ring-1 ring-tx-0',
          selected && 'z-10 ring-2 ring-indigo ring-offset-1 ring-offset-bg-0',
          ancestor && 'saturate-50',
        )}
        style={{
          left: `${left}%`,
          width: `${width}%`,
          top: node.depth * ROW_HEIGHT,
          height: ROW_HEIGHT - 1,
          ...barStyle(node),
        }}
      >
        {width >= LABEL_MIN_PCT && <span className="truncate">{name}</span>}
      </button>
    );
  };

  const bars: React.ReactNode[] = [];
  for (let depth = 0; depth < depthCount; depth += 1) {
    const rowNodes = byDepth[depth] ?? [];
    if (depth < focusWin.depth) {
      const ancestor = ancestorAtDepth(nodes, focusWin, depth);
      if (ancestor) bars.push(renderBar(ancestor, `ancestor-${depth}`, 0, 100, true));
      continue;
    }
    for (const node of rowNodes) {
      if (!nodeInWindow(node, focusWin)) continue;
      const left = ((node.start - focusWin.start) / focusWin.total) * 100;
      const width = (node.total / focusWin.total) * 100;
      if (width < MIN_BAR_PCT) continue;
      bars.push(
        renderBar(
          node,
          `${depth}-${node.start}`,
          Math.max(0, left),
          Math.min(100, width),
          false,
        ),
      );
    }
  }

  const zoomed = focus !== null && focus.depth > 0;
  const flameHeight = depthCount * ROW_HEIGHT;
  const viewportHeight = fullscreen ? 'calc(100vh - 300px)' : analysisHeight;

  return (
    <div
      ref={rootRef}
      className={cn(
        'flex flex-col overflow-hidden rounded-md border border-bd-0 bg-bg-1',
        fullscreen && 'h-screen rounded-none border-0 bg-bg-0 p-4',
        className,
      )}
    >
      <div className="flex min-h-12 flex-wrap items-center gap-2 border-b border-bd-0 px-3 py-2">
        {!diff && <ViewSwitcher view={view} onChange={setView} />}
        <div className="relative min-w-[220px] flex-1 sm:max-w-[320px]">
          <Search className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-tx-3" />
          <Input
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder={t('flamegraph.search_placeholder')}
            aria-label={t('flamegraph.search_placeholder')}
            className="h-8 pl-8 font-sans text-xs"
          />
        </div>
        {headerExtra && (
          <span className="whitespace-nowrap font-sans text-xs text-tx-2">
            {headerExtra}
          </span>
        )}
        <div className="ml-auto flex items-center gap-1">
          {view === 'flame' && zoomed && (
            <Button variant="outline" size="sm" onClick={() => setFocus(null)}>
              <ZoomOut className="h-3.5 w-3.5" /> {t('flamegraph.reset_zoom')}
            </Button>
          )}
          <Button
            variant="outline"
            size="icon"
            onClick={() => {
              setFocus(null);
              setQuery('');
            }}
            aria-label={t('flamegraph.fit_canvas')}
            title={t('flamegraph.fit_canvas')}
          >
            <RotateCcw className="h-3.5 w-3.5" />
          </Button>
          <Button
            variant="outline"
            size="icon"
            onClick={toggleFullscreen}
            aria-label={fullscreen ? t('flamegraph.exit_fullscreen') : t('flamegraph.fullscreen')}
            title={fullscreen ? t('flamegraph.exit_fullscreen') : t('flamegraph.fullscreen')}
          >
            {fullscreen ? <Minimize2 className="h-3.5 w-3.5" /> : <Maximize2 className="h-3.5 w-3.5" />}
          </Button>
        </div>
      </div>

      <div className="flex min-h-9 flex-wrap items-center border-b border-bd-0 bg-bg-2 px-3 py-1.5">
        {diff ? <DiffLegend /> : <HeatLegend />}
      </div>

      <div
        className="min-h-0 overflow-auto bg-bg-0"
        style={{ height: viewportHeight }}
      >
        {view === 'top' && (
          <TopFunctionsTable
            rows={functionRows}
            units={units}
            needle={needle}
            selectedName={activeFunction}
            onSelect={selectFunction}
          />
        )}

        {view === 'tree' && (
          <CallTreeView
            nodes={nodes}
            names={names}
            total={total}
            units={units}
            needle={needle}
            selectedKey={selectedNode ? nodeKey(selectedNode) : null}
            selectedPathKeys={selectedPathKeys}
            onSelect={selectNode}
          />
        )}

        {view === 'flame' && (
          <div
            ref={containerRef}
            onMouseMove={handleMove}
            onMouseLeave={() => setHoverNode(null)}
            className="relative min-h-full p-1"
          >
            <div
              className="relative min-h-full"
              style={{ height: Math.max(flameHeight, analysisHeight) }}
            >
              {bars}
            </div>
            {hoverNode && (
              <FlameTooltip
                node={hoverNode}
                name={nameOf(hoverNode)}
                total={total}
                units={units}
                diff={diff}
                pointer={pointer}
                service={service}
              />
            )}
          </div>
        )}
      </div>

      {!fullscreen && (
        <div
          role="separator"
          aria-orientation="horizontal"
          aria-label={t('flamegraph.resize')}
          title={t('flamegraph.resize')}
          className="group flex h-2 shrink-0 cursor-row-resize items-center justify-center border-y border-bd-0 bg-bg-1 hover:bg-bg-2"
          onMouseDown={handleResizeStart}
          onDoubleClick={() => setAnalysisHeight(DEFAULT_ANALYSIS_HEIGHT)}
        >
          <span className="h-px w-9 rounded-full bg-bd-2 group-hover:bg-indigo" />
        </div>
      )}

      {!diff && selectedNode && selectedStats && (
        <FunctionInsightPanel
          stats={selectedStats}
          path={selectedPath}
          names={names}
          childFunctions={selectedChildren}
          occurrences={selectedOccurrences}
          units={units}
          service={service}
          onFocus={() => focusFunction(selectedStats.name)}
          onTree={() => {
            selectFunction(selectedStats.name);
            setView('tree');
          }}
          onCompare={onCompare}
          onPathSelect={(node) => {
            setView('flame');
            setQuery('');
            selectNode(node, true);
          }}
          onChildSelect={focusFunction}
        />
      )}
    </div>
  );
}

function ViewSwitcher({
  view,
  onChange,
}: {
  view: ProfileView;
  onChange: (view: ProfileView) => void;
}) {
  const { t } = useTranslation('profiles');
  const items: Array<{ id: ProfileView; icon: typeof Flame; label: string }> = [
    { id: 'flame', icon: Flame, label: t('flamegraph.view.flame') },
    { id: 'tree', icon: ListTree, label: t('flamegraph.view.tree') },
    { id: 'top', icon: Table2, label: t('flamegraph.view.top') },
  ];
  return (
    <div className="inline-flex h-8 items-center gap-0.5 rounded-md border border-bd-1 bg-bg-2 p-0.5">
      {items.map((item) => {
        const Icon = item.icon;
        return (
          <button
            key={item.id}
            type="button"
            onClick={() => onChange(item.id)}
            className={cn(
              'inline-flex h-7 items-center gap-1 rounded px-2 font-sans text-xs transition-colors',
              view === item.id
                ? 'bg-bg-0 font-semibold text-tx-0 shadow-sm'
                : 'text-tx-2 hover:text-tx-0',
            )}
            aria-pressed={view === item.id}
          >
            <Icon className="h-3 w-3" /> {item.label}
          </button>
        );
      })}
    </div>
  );
}

type TopSort = 'self' | 'total';

function TopFunctionsTable({
  rows,
  units,
  needle,
  selectedName,
  onSelect,
}: {
  rows: TopFunction[];
  units: string;
  needle: string;
  selectedName: string | null;
  onSelect: (name: string) => void;
}) {
  const { t } = useTranslation('profiles');
  const [sort, setSort] = React.useState<TopSort>('self');
  const filtered = needle
    ? rows.filter((row) => row.name.toLowerCase().includes(needle))
    : rows;
  const shown = [...filtered]
    .sort((a, b) => b[sort] - a[sort])
    .slice(0, 100);
  const maxSelf = Math.max(1, ...shown.map((row) => row.self));

  if (shown.length === 0) {
    return (
      <div className="p-8 text-center font-sans text-xs text-tx-2">
        {t('flamegraph.no_data_title')}
      </div>
    );
  }

  return (
    <div>
      <div className="sticky top-0 z-20 flex min-h-10 items-center gap-2 border-b border-bd-0 bg-bg-1 px-3">
        <span className="font-sans text-xs text-tx-3">{t('flamegraph.top.sort_label')}</span>
        <div className="inline-flex rounded-md border border-bd-1 bg-bg-2 p-0.5">
          {(['self', 'total'] as const).map((item) => (
            <button
              key={item}
              type="button"
              onClick={() => setSort(item)}
              className={cn(
                'h-7 rounded px-2 font-sans text-xs',
                sort === item ? 'bg-bg-0 font-semibold text-tx-0' : 'text-tx-2 hover:text-tx-0',
              )}
              aria-pressed={sort === item}
            >
              {item === 'self' ? t('flamegraph.top.self') : t('flamegraph.top.cum')}
            </button>
          ))}
        </div>
      </div>
      <table className="w-full border-collapse font-sans text-xs">
        <thead className="sticky top-10 z-10 bg-bg-1">
          <tr className="border-b border-bd-0 text-left font-strong text-tx-3 [&_th]:px-3 [&_th]:py-2 [&_th]:text-xs">
            <th className="w-8">#</th>
            <th>{t('flamegraph.top.function')}</th>
            <th className="w-48" aria-sort={sort === 'self' ? 'descending' : undefined}>
              {t('flamegraph.top.self')}{sort === 'self' ? ' ↓' : ''}
            </th>
            <th className="w-20 text-right">{t('flamegraph.top.self_pct')}</th>
            <th className="w-24 text-right" aria-sort={sort === 'total' ? 'descending' : undefined}>
              {t('flamegraph.top.cum')}{sort === 'total' ? ' ↓' : ''}
            </th>
            <th className="w-20 text-right">{t('flamegraph.top.cum_pct')}</th>
          </tr>
        </thead>
        <tbody>
          {shown.map((row, index) => {
            const selected = selectedName === row.name;
            return (
              <tr
                key={row.name}
                role="button"
                tabIndex={0}
                onClick={() => onSelect(row.name)}
                onKeyDown={(event) => {
                  if (event.key === 'Enter' || event.key === ' ') {
                    event.preventDefault();
                    onSelect(row.name);
                  }
                }}
                className={cn(
                  'cursor-pointer border-b border-bd-0 hover:bg-bg-2 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-indigo [&_td]:px-3 [&_td]:py-2',
                  selected && 'bg-indigo-dim',
                )}
                title={t('flamegraph.top.select_hint')}
              >
                <td className="tabular-nums text-tx-3">{index + 1}</td>
                <td className="max-w-0 truncate font-mono text-xs text-tx-0">{row.name}</td>
                <td>
                  <div className="flex items-center gap-2">
                    <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-bg-3">
                      <div
                        className="h-full rounded-full"
                        style={{
                          width: `${(row.self / maxSelf) * 100}%`,
                          background: heatColor(row.self / maxSelf),
                        }}
                      />
                    </div>
                    <span className="w-16 shrink-0 text-right tabular-nums text-tx-1">
                      {formatValue(row.self, units)}
                    </span>
                  </div>
                </td>
                <td className="text-right tabular-nums text-tx-2">
                  {row.selfPct.toFixed(1)}%
                </td>
                <td className="text-right tabular-nums text-tx-1">
                  {formatValue(row.total, units)}
                </td>
                <td className="text-right tabular-nums text-tx-2">
                  {row.totalPct.toFixed(1)}%
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

function CallTreeView({
  nodes,
  names,
  total,
  units,
  needle,
  selectedKey,
  selectedPathKeys,
  onSelect,
}: {
  nodes: FlameNode[];
  names: string[];
  total: number;
  units: string;
  needle: string;
  selectedKey: string | null;
  selectedPathKeys: Set<string>;
  onSelect: (node: FlameNode) => void;
}) {
  const { t } = useTranslation('profiles');
  const children = React.useMemo(() => callTreeChildren(nodes), [nodes]);
  const [collapsed, setCollapsed] = React.useState<Set<string>>(new Set());
  const toggle = (key: string) =>
    setCollapsed((previous) => {
      const next = new Set(previous);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });

  const rows: React.ReactNode[] = [];
  const walk = (node: FlameNode, depth: number) => {
    const key = nodeKey(node);
    const childNodes = children.get(key) ?? [];
    const name = names[node.nameIndex] ?? '';
    const pct = total > 0 ? (node.total / total) * 100 : 0;
    const matched = needle.length > 0 && name.toLowerCase().includes(needle);
    const isCollapsed = collapsed.has(key);
    const selected = selectedKey === key;
    const inPath = selectedPathKeys.has(key);
    rows.push(
      <div
        key={key}
        className={cn(
          'flex min-h-9 items-center gap-2 border-b border-bd-0 px-2 font-sans text-xs hover:bg-bg-2',
          matched && 'bg-yellow-dim',
          inPath && !selected && 'bg-blue-dim',
          selected && 'border-l-2 border-l-indigo bg-indigo-dim',
        )}
      >
        <span
          className="relative h-8 shrink-0"
          style={{
            width: depth * 16 + 18,
            backgroundImage:
              depth > 0
                ? 'repeating-linear-gradient(to right, transparent 0, transparent 14px, var(--bd-0) 14px, var(--bd-0) 15px, transparent 15px, transparent 16px)'
                : undefined,
          }}
        >
          <button
            type="button"
            onClick={() => childNodes.length > 0 && toggle(key)}
            className={cn(
              'absolute right-0 top-1/2 grid h-5 w-5 -translate-y-1/2 place-items-center rounded text-tx-3 hover:bg-bg-3 hover:text-tx-0',
              childNodes.length === 0 && 'invisible',
            )}
            aria-label={isCollapsed ? t('flamegraph.tree.expand') : t('flamegraph.tree.collapse')}
          >
            {isCollapsed ? <ChevronRight className="h-3.5 w-3.5" /> : <ChevronDown className="h-3.5 w-3.5" />}
          </button>
        </span>
        <button
          type="button"
          onClick={() => onSelect(node)}
          className="min-w-0 flex-1 truncate text-left font-mono text-xs text-tx-0"
          title={name}
        >
          {name}
        </button>
        <span className="w-20 shrink-0 text-right tabular-nums text-tx-2">
          {formatValue(node.self, units)}
        </span>
        <span className="w-24 shrink-0 text-right tabular-nums text-tx-1">
          {formatValue(node.total, units)}
        </span>
        <span className="w-20 shrink-0 text-right tabular-nums text-tx-3">
          {pct.toFixed(1)}%
        </span>
      </div>,
    );
    if (!isCollapsed) {
      for (const child of childNodes) walk(child, depth + 1);
    }
  };
  for (const root of children.get('root') ?? []) walk(root, 0);

  return (
    <div>
      <div className="sticky top-0 z-10 flex min-h-10 items-center gap-2 border-b border-bd-0 bg-bg-1 px-2 font-sans text-xs font-strong text-tx-3">
        <span className="h-4 w-[18px] shrink-0" />
        <span className="min-w-0 flex-1">{t('flamegraph.top.function')}</span>
        <span className="w-20 shrink-0 text-right" title={t('flamegraph.self_help')}>
          {t('flamegraph.frame_self')}
        </span>
        <span className="w-24 shrink-0 text-right" title={t('flamegraph.total_help')}>
          {t('flamegraph.frame_total')}
        </span>
        <span className="w-20 shrink-0 text-right">{t('flamegraph.frame_share')}</span>
      </div>
      {rows}
    </div>
  );
}

function FunctionInsightPanel({
  stats,
  path,
  names,
  childFunctions,
  occurrences,
  units,
  service,
  onFocus,
  onTree,
  onCompare,
  onPathSelect,
  onChildSelect,
}: {
  stats: TopFunction;
  path: FlameNode[];
  names: string[];
  childFunctions: ChildFunction[];
  occurrences: number;
  units: string;
  service?: string | undefined;
  onFocus: () => void;
  onTree: () => void;
  onCompare?: (() => void) | undefined;
  onPathSelect: (node: FlameNode) => void;
  onChildSelect: (name: string) => void;
}) {
  const { t } = useTranslation('profiles');
  return (
    <div className="grid border-t border-bd-0 bg-bg-1 lg:grid-cols-[minmax(0,1.45fr)_minmax(300px,0.55fr)]">
      <section className="min-w-0 p-4 lg:border-r lg:border-bd-0">
        <div className="flex min-w-0 flex-wrap items-start gap-3">
          <div className="min-w-0 flex-1">
            <div className="font-sans text-xs font-semibold text-tx-3">
              {t('flamegraph.details.title')}
            </div>
            <div className="mt-1 truncate font-mono text-sm font-semibold text-tx-0" title={stats.name}>
              {stats.name}
            </div>
          </div>
          <div className="flex shrink-0 flex-wrap gap-1">
            <Button variant="outline" size="sm" onClick={onFocus}>
              <Flame className="h-3.5 w-3.5" /> {t('flamegraph.details.focus')}
            </Button>
            <Button variant="outline" size="sm" onClick={onTree}>
              <ListTree className="h-3.5 w-3.5" /> {t('flamegraph.details.call_tree')}
            </Button>
            {onCompare && (
              <Button variant="outline" size="sm" onClick={onCompare}>
                <GitCompare className="h-3.5 w-3.5" /> {t('flamegraph.details.compare')}
              </Button>
            )}
          </div>
        </div>

        <dl className="mt-4 grid grid-cols-2 gap-x-6 gap-y-3 sm:grid-cols-4">
          <InsightMetric
            label={t('flamegraph.frame_self')}
            value={formatValue(stats.self, units)}
            sub={`${stats.selfPct.toFixed(1)}%`}
          />
          <InsightMetric
            label={t('flamegraph.frame_total')}
            value={formatValue(stats.total, units)}
            sub={`${stats.totalPct.toFixed(1)}%`}
          />
          <InsightMetric
            label={t('flamegraph.details.occurrences')}
            value={String(occurrences)}
          />
          <InsightMetric
            label={t('flamegraph.details.service')}
            value={service || t('flamegraph.details.all_services')}
          />
        </dl>

        <div className="mt-4">
          <div className="font-sans text-xs font-semibold text-tx-3">
            {t('flamegraph.details.call_path')}
          </div>
          <div className="mt-2 flex min-w-0 flex-wrap items-center gap-1">
            {path.map((node, index) => {
              const name = names[node.nameIndex] ?? '';
              return (
                <React.Fragment key={nodeKey(node)}>
                  {index > 0 && <ChevronRight className="h-3 w-3 shrink-0 text-tx-4" />}
                  <button
                    type="button"
                    onClick={() => onPathSelect(node)}
                    className="max-w-[220px] truncate rounded border border-bd-0 bg-bg-2 px-2 py-1 font-mono text-xs text-tx-1 hover:border-bd-1 hover:text-tx-0"
                    title={name}
                  >
                    {name}
                  </button>
                </React.Fragment>
              );
            })}
          </div>
        </div>
      </section>

      <section className="min-w-0 p-4">
        <div className="font-sans text-xs font-semibold text-tx-3">
          {t('flamegraph.details.children')}
        </div>
        {childFunctions.length > 0 ? (
          <div className="mt-2 space-y-1.5">
            {childFunctions.slice(0, 5).map((child) => (
              <button
                key={child.name}
                type="button"
                onClick={() => onChildSelect(child.name)}
                className="grid w-full grid-cols-[minmax(0,1fr)_auto] items-center gap-3 rounded px-2 py-1.5 text-left hover:bg-bg-2"
                title={child.name}
              >
                <span className="truncate font-mono text-xs text-tx-1">{child.name}</span>
                <span className="font-sans text-xs tabular-nums text-tx-2">
                  {child.pct.toFixed(1)}%
                </span>
              </button>
            ))}
          </div>
        ) : (
          <div className="mt-3 font-sans text-xs text-tx-3">
            {t('flamegraph.details.no_children')}
          </div>
        )}
      </section>
    </div>
  );
}

function InsightMetric({
  label,
  value,
  sub,
}: {
  label: string;
  value: string;
  sub?: string | undefined;
}) {
  return (
    <div className="min-w-0">
      <dt className="font-sans text-xs text-tx-3">{label}</dt>
      <dd className="mt-1 truncate font-mono text-sm font-semibold tabular-nums text-tx-0" title={value}>
        {value}
      </dd>
      {sub && <div className="mt-0.5 font-sans text-xs text-tx-2">{sub}</div>}
    </div>
  );
}

function FlameTooltip({
  node,
  name,
  total,
  units,
  diff,
  pointer,
  service,
}: {
  node: FlameNode;
  name: string;
  total: number;
  units: string;
  diff: boolean;
  pointer: PointerPosition;
  service?: string | undefined;
}) {
  const { t } = useTranslation('profiles');
  const totalShare = total > 0 ? (node.total / total) * 100 : 0;
  const selfShare = total > 0 ? (node.self / total) * 100 : 0;
  const left = Math.max(8, Math.min(pointer.x + 12, pointer.width - 332));
  const top = pointer.y + 190 > pointer.height
    ? Math.max(8, pointer.y - 166)
    : pointer.y + 12;
  return (
    <div
      className="pointer-events-none absolute z-20 w-[320px] max-w-[calc(100%-16px)] rounded-md border border-bd-1 bg-surface px-3 py-2.5 shadow-popup"
      style={{ left, top }}
    >
      <div className="mb-2 break-all font-mono text-xs font-semibold text-tx-0">{name}</div>
      <div className="grid grid-cols-[1fr_auto] gap-x-4 gap-y-1 font-sans text-xs tabular-nums text-tx-2">
        <span>{t('flamegraph.frame_self')}</span>
        <span className="text-right text-tx-1">
          {formatValue(node.self, units)} · {selfShare.toFixed(1)}%
        </span>
        <span>{t('flamegraph.frame_total')}</span>
        <span className="text-right text-tx-1">
          {formatValue(node.total, units)} · {totalShare.toFixed(1)}%
        </span>
        <span>{t('flamegraph.details.depth')}</span>
        <span className="text-right text-tx-1">{node.depth}</span>
        {service && (
          <>
            <span>{t('flamegraph.details.service')}</span>
            <span className="max-w-[180px] truncate text-right text-tx-1">{service}</span>
          </>
        )}
        {diff && node.delta !== undefined && (
          <>
            <span>Δ</span>
            <span className={cn('text-right', node.delta > 0 ? 'text-red' : node.delta < 0 ? 'text-green' : 'text-tx-1')}>
              {node.delta > 0 ? '+' : ''}
              {formatValue(node.delta, units)}
            </span>
          </>
        )}
      </div>
      <div className="mt-2 border-t border-bd-0 pt-2 font-sans text-xs text-tx-3">
        {t('flamegraph.tooltip.focus_hint')}
      </div>
    </div>
  );
}

function HeatLegend() {
  const { t } = useTranslation('profiles');
  return (
    <div className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1 font-sans text-xs text-tx-3">
      <span>
        <strong className="font-semibold text-tx-2">{t('flamegraph.legend.width')}</strong>
        {' = '}
        {t('flamegraph.legend.cumulative')}
      </span>
      <span className="hidden h-3 w-px bg-bd-1 sm:block" />
      <span className="flex items-center gap-1.5">
        <strong className="font-semibold text-tx-2">{t('flamegraph.legend.color')}</strong>
        {' = '}
        {t('flamegraph.legend.self')}
        <span
          className="h-2.5 w-16 rounded-full"
          style={{
            background: `linear-gradient(to right, ${heatColor(0)}, ${heatColor(0.5)}, ${heatColor(1)})`,
          }}
        />
        <span>{t('flamegraph.legend.cold')}</span>
        <ChevronRight className="h-3 w-3" />
        <span className="text-tx-2">{t('flamegraph.legend.hot')}</span>
      </span>
    </div>
  );
}

function DiffLegend() {
  const { t } = useTranslation('profiles');
  return (
    <div className="flex items-center gap-3 font-sans text-xs text-tx-2">
      <span className="flex items-center gap-1">
        <span className="h-2.5 w-2.5 rounded-[2px] bg-red" /> {t('compare.legend_increase')}
      </span>
      <span className="flex items-center gap-1">
        <span className="h-2.5 w-2.5 rounded-[2px] bg-green" /> {t('compare.legend_decrease')}
      </span>
    </div>
  );
}

interface ChildFunction {
  name: string;
  total: number;
  self: number;
  pct: number;
}

function hottestNodeForName(
  nodes: FlameNode[],
  names: string[],
  name: string | null,
): FlameNode | null {
  if (!name) return null;
  return nodes
    .filter((node) => names[node.nameIndex] === name)
    .sort((a, b) => b.self - a.self || b.total - a.total)[0] ?? null;
}

function callPath(nodes: FlameNode[], selected: FlameNode): FlameNode[] {
  const path: FlameNode[] = [];
  for (let depth = 0; depth < selected.depth; depth += 1) {
    const ancestor = ancestorAtDepth(
      nodes,
      { depth: selected.depth, start: selected.start, total: selected.total },
      depth,
    );
    if (ancestor) path.push(ancestor);
  }
  path.push(selected);
  return path;
}

function aggregateChildren(
  childNodes: FlameNode[],
  names: string[],
  parentTotal: number,
): ChildFunction[] {
  const aggregated = new Map<string, { self: number; total: number }>();
  for (const node of childNodes) {
    const name = names[node.nameIndex] ?? '';
    if (!name) continue;
    const current = aggregated.get(name) ?? { self: 0, total: 0 };
    current.self += node.self;
    current.total += node.total;
    aggregated.set(name, current);
  }
  return [...aggregated.entries()]
    .map(([name, value]) => ({
      name,
      self: value.self,
      total: value.total,
      pct: parentTotal > 0 ? (value.total / parentTotal) * 100 : 0,
    }))
    .sort((a, b) => b.total - a.total);
}

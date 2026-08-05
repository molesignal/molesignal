import { keepPreviousData, useQueries } from '@tanstack/react-query';
import {
  Bot,
  ChevronDown,
  ChevronRight,
  Copy,
  Download,
  Ellipsis,
  ExternalLink,
  Eye,
  FileCode2,
  Fullscreen,
  ImageDown,
  Pencil,
  RefreshCw,
  Share2,
  Trash2,
} from 'lucide-react';
import * as React from 'react';
import { useNavigate } from 'react-router-dom';

import * as dashboardsApi from '@/api/dashboards';
import {
  canAccessProductPath,
  useProductAccess,
} from '@/product/access';
import { ChromeButton } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from '@/shell/ui/dialog';
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';
import { toast } from '@/shell/ui/sonner';
import {
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
} from '@/shell/ui/tabs';
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/shell/ui/tooltip';

import { resolveInlineAnnotations } from './annotations';
import { buildDataLinkUrl } from './dataLinks';
import {
  EditableDashboardGridItem,
  type DashboardGridEditingConfig,
} from './editor/EditableDashboardGridItem';
import { useDashboardText } from './i18n';
import { gridSpanSize } from './layout';
import { PanelDescriptionTooltip } from './PanelDescriptionTooltip';
import {
  buildDashboardQueryLookup,
  usePanelData,
  type DashboardPanelQueryExecutor,
  type PanelQueryRuntimeContext,
} from './query/usePanelData';
import {
  refreshCadenceFromSettings,
  resolveRefreshIntervalMilliseconds,
  type DashboardRefreshCadence,
  type DashboardTimeRangeResolver,
} from './refresh/policy';
import { useDashboardRefresh } from './refresh/useDashboardRefresh';
import { DEFAULT_DASHBOARD_CURSOR_SYNC_MODE } from './schema';
import type {
  DashboardDefinition,
  DashboardGroup,
  DashboardPanel,
  DashboardRow,
  DashboardTab,
  DashboardTextElement,
  DashboardTimeRange,
  DashboardVariable,
  PanelData,
  PanelQuery,
} from './schema';
import {
  expandRepeatedElements,
  initialVariableValues,
  interpolateVariables,
  variableOptions,
  type DashboardVariableValues,
  type RuntimeDashboardElement,
} from './variables';
import {
  VisualizationRenderer,
  visualizationRegistry,
} from './visualizations';

export interface DashboardRendererProps {
  dashboard: DashboardDefinition;
  orgId: string;
  className?: string;
  refreshNonce?: number;
  refreshIntervalOverride?: DashboardRefreshCadence;
  onRenderStateChange?: (state: 'loading' | 'ready') => void;
  onRefreshStateChange?: (refreshing: boolean) => void;
  onEditPanel?: (panelId: string) => void;
  onDuplicatePanel?: (panelId: string) => void;
  onRemovePanel?: (panelId: string) => void;
  editMode?: DashboardGridEditingConfig;
  panelQueryExecutor?: DashboardPanelQueryExecutor;
  restricted?: boolean;
  variableControlsEnabled?: boolean;
  resolveVariableQueries?: boolean;
  maxTimeRangeMicros?: number;
}

export type { DashboardPanelQueryExecutor } from './query/usePanelData';

interface DashboardRenderContext extends PanelQueryRuntimeContext {
  dashboard: DashboardDefinition;
  canCreateAlert: boolean;
  canUseMoleAgent: boolean;
  onEditPanel?: (panelId: string) => void;
  onDuplicatePanel?: (panelId: string) => void;
  onRemovePanel?: (panelId: string) => void;
  panelQueryExecutor?: DashboardPanelQueryExecutor;
  restricted: boolean;
  maxTimeRangeMicros?: number;
}

export function DashboardRenderer({
  dashboard,
  orgId,
  className,
  refreshNonce = 0,
  refreshIntervalOverride,
  onRenderStateChange,
  onRefreshStateChange,
  onEditPanel,
  onDuplicatePanel,
  onRemovePanel,
  editMode,
  panelQueryExecutor,
  restricted = false,
  variableControlsEnabled = true,
  resolveVariableQueries = true,
  maxTimeRangeMicros,
}: DashboardRendererProps) {
  const tr = useDashboardText();
  const productAccess = useProductAccess();
  const canCreateAlert = canAccessProductPath(
    '/alerts/rules/new',
    productAccess,
  );
  const canUseMoleAgent = canAccessProductPath(
    '/intelligence/chat',
    productAccess,
  );
  const configuredRefreshCadence = refreshCadenceFromSettings(
    dashboard.refreshSettings,
  );
  const refreshCadence =
    refreshIntervalOverride === undefined
      ? configuredRefreshCadence
      : refreshIntervalOverride;
  const refreshRuntime = useDashboardRefresh({
    dashboardUid: dashboard.uid,
    refreshNonce,
    maxTimeRangeMicros,
    onRenderStateChange,
    onRefreshStateChange,
  });
  const [variables, setVariables] = React.useState<DashboardVariableValues>(
    () => initialVariableValues(dashboard.variables),
  );
  React.useEffect(() => {
    setVariables(initialVariableValues(dashboard.variables));
  }, [dashboard.uid, dashboard.variables]);
  const queryLookup = React.useMemo(
    () => buildDashboardQueryLookup(dashboard.elements),
    [dashboard.elements],
  );
  const context = React.useMemo<DashboardRenderContext>(
    () => ({
      dashboard,
      canCreateAlert,
      canUseMoleAgent,
      dashboardUid: dashboard.uid,
      orgId,
      timeRange: refreshRuntime.timeRange,
      timeRangeKey: refreshRuntime.timeRangeKey,
      resolveTimeRange: refreshRuntime.resolveTimeRange,
      refreshCadence,
      containerWidth: refreshRuntime.containerWidth,
      dashboardColumns: dashboard.layout.columns,
      queryLookup,
      restricted,
      ...(onEditPanel ? { onEditPanel } : {}),
      ...(onDuplicatePanel ? { onDuplicatePanel } : {}),
      ...(onRemovePanel ? { onRemovePanel } : {}),
      ...(panelQueryExecutor ? { panelQueryExecutor } : {}),
      ...(maxTimeRangeMicros !== undefined ? { maxTimeRangeMicros } : {}),
    }),
    [
      dashboard,
      canCreateAlert,
      canUseMoleAgent,
      onDuplicatePanel,
      onEditPanel,
      onRemovePanel,
      orgId,
      panelQueryExecutor,
      queryLookup,
      refreshCadence,
      refreshRuntime.containerWidth,
      refreshRuntime.resolveTimeRange,
      refreshRuntime.timeRange,
      refreshRuntime.timeRangeKey,
      restricted,
      maxTimeRangeMicros,
    ],
  );
  const elements = React.useMemo(
    () =>
      expandRepeatedElements(
        dashboard.elements,
        variables,
        dashboard.layout.columns,
      ),
    [dashboard.elements, dashboard.layout.columns, variables],
  );
  return (
    <TooltipProvider delayDuration={200}>
      <div
        ref={refreshRuntime.containerRef}
        className={cn('space-y-3', className)}
      >
        <DashboardVariableBar
          dashboardUid={dashboard.uid}
          variables={dashboard.variables}
          values={variables}
          timeRange={refreshRuntime.timeRange}
          timeRangeKey={refreshRuntime.timeRangeKey}
          resolveTimeRange={refreshRuntime.resolveTimeRange}
          refreshCadence={refreshCadence}
          containerWidth={refreshRuntime.containerWidth}
          onChange={setVariables}
          disabled={!variableControlsEnabled}
          resolveQueries={resolveVariableQueries}
        />
        {!restricted && (
          <DashboardLinksBar
            dashboard={dashboard}
            variables={variables}
            timeRange={refreshRuntime.timeRange}
          />
        )}
        {elements.length === 0 ? (
          <div className="grid min-h-[48vh] place-items-center rounded-md border border-dashed border-bd-1 bg-bg-1 font-sans text-sm text-tx-3">
            {tr('This dashboard has no elements')}
          </div>
        ) : (
          <DashboardGrid
            elements={elements}
            variables={variables}
            context={context}
            columns={dashboard.layout.columns}
            rowHeight={dashboard.layout.rowHeight}
            gap={dashboard.layout.gap}
            {...(editMode ? { editMode } : {})}
          />
        )}
      </div>
    </TooltipProvider>
  );
}

function DashboardLinksBar({
  dashboard,
  variables,
  timeRange,
}: {
  dashboard: DashboardDefinition;
  variables: DashboardVariableValues;
  timeRange: DashboardTimeRange;
}) {
  const tr = useDashboardText();
  if (dashboard.links.length === 0) return null;
  return (
    <nav
      className="flex flex-wrap items-center gap-1.5"
      aria-label={tr('Dashboard links')}
    >
      {dashboard.links.map((link) => {
        const href = dashboardLinkUrl(link.url, {
          variables,
          timeRange,
          includeTimeRange: link.includeTimeRange,
          includeVariables: link.includeVariables,
        });
        return (
          <a
            key={link.id}
            href={href}
            target={link.openInNewTab ? '_blank' : undefined}
            rel={link.openInNewTab ? 'noreferrer' : undefined}
            className="inline-flex h-7 items-center gap-1 rounded-md border border-bd-0 bg-bg-1 px-2 font-sans text-xs text-tx-2 transition-colors hover:border-bd-2 hover:text-tx-0"
          >
            {link.title}
            {link.openInNewTab && <ExternalLink className="h-3 w-3" />}
          </a>
        );
      })}
    </nav>
  );
}

function DashboardGrid({
  elements,
  variables,
  context,
  columns,
  rowHeight,
  gap,
  nested = false,
  editMode,
}: {
  elements: readonly RuntimeDashboardElement[];
  variables: DashboardVariableValues;
  context: DashboardRenderContext;
  columns: number;
  rowHeight: number;
  gap: number;
  nested?: boolean;
  editMode?: DashboardGridEditingConfig;
}) {
  return (
    <div className={cn('overflow-x-auto', nested && 'h-full')}>
      <div
        data-dashboard-editor-grid={editMode ? '' : undefined}
        className="grid min-w-[720px] items-stretch"
        style={{
          gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))`,
          gridAutoRows: `${rowHeight}px`,
          gap: `${gap}px`,
        }}
      >
        {elements.map((runtime) => {
          const style = {
            gridColumn: `${runtime.element.gridPos.x + 1} / span ${runtime.element.gridPos.w}`,
            gridRow: `${runtime.element.gridPos.y + 1} / span ${runtime.element.gridPos.h}`,
          };
          const renderedElement = (
            <DashboardElementRenderer
              runtime={runtime}
              variables={runtime.variables ?? variables}
              context={context}
              rowHeight={rowHeight}
              gap={gap}
            />
          );
          if (!editMode) {
            return (
              <div
                key={runtime.key}
                className="min-h-0 min-w-0"
                style={style}
              >
                {renderedElement}
              </div>
            );
          }
          const elementId = baseElementId(runtime.element.id);
          return (
            <EditableDashboardGridItem
              key={runtime.key}
              element={runtime.element}
              elementId={elementId}
              editing={editMode}
              style={style}
            >
              {renderedElement}
            </EditableDashboardGridItem>
          );
        })}
      </div>
    </div>
  );
}

function DashboardElementRenderer({
  runtime,
  variables,
  context,
  rowHeight,
  gap,
}: {
  runtime: RuntimeDashboardElement;
  variables: DashboardVariableValues;
  context: DashboardRenderContext;
  rowHeight: number;
  gap: number;
}) {
  const element = runtime.element;
  if (element.kind === 'panel') {
    return (
      <DashboardPanelCard
        panel={element}
        variables={variables}
        context={context}
        height={gridSpanSize(element.gridPos.h, rowHeight, gap)}
      />
    );
  }
  if (element.kind === 'text') return <DashboardTextCard element={element} />;
  if (element.kind === 'tab') {
    return (
      <DashboardTabCard
        element={element}
        variables={variables}
        context={context}
        rowHeight={rowHeight}
        gap={gap}
      />
    );
  }
  return (
    <DashboardContainerCard
      element={element}
      variables={variables}
      context={context}
      rowHeight={rowHeight}
      gap={gap}
    />
  );
}

function DashboardPanelCard({
  panel,
  variables,
  context,
  height,
}: {
  panel: DashboardPanel;
  variables: DashboardVariableValues;
  context: DashboardRenderContext;
  height: number;
}) {
  const tr = useDashboardText();
  const nav = useNavigate();
  const [fullscreen, setFullscreen] = React.useState(false);
  const [queryInspector, setQueryInspector] = React.useState(false);
  const panelRef = React.useRef<HTMLElement>(null);
  const cursorScopeId =
    (context.dashboard.interactionSettings?.cursorSync ??
      DEFAULT_DASHBOARD_CURSOR_SYNC_MODE) === 'off'
      ? null
      : `dashboard:${context.dashboard.uid}`;
  const data = usePanelData(panel, variables, context);
  const annotations = React.useMemo(
    () =>
      resolveInlineAnnotations(
        context.dashboard.annotations,
        variables,
        data.timeRange,
      ),
    [context.dashboard.annotations, data.timeRange, variables],
  );
  const renderPanel = React.useMemo(
    () =>
      panel.visualization.type === 'time_series' && annotations.length > 0
        ? {
            ...panel,
            visualization: {
              ...panel.visualization,
              options: {
                ...panel.visualization.options,
                annotations: [
                  ...(Array.isArray(panel.visualization.options.annotations)
                    ? panel.visualization.options.annotations
                    : []),
                  ...annotations,
                ],
              },
            },
          }
        : panel,
    [annotations, panel],
  );
  const title = interpolateVariables(panel.title, variables);
  const description = panel.description
    ? interpolateVariables(panel.description, variables)
    : '';
  const activeQuery = panel.queries.find((query) => query.enabled);
  const exploreRoute = activeQuery
    ? signalExploreRoute(activeQuery.dataSourceType)
    : '/metrics';
  const menuButton = (
    <button
      type="button"
      aria-label={`${tr('Open panel menu')}: ${title}`}
      className="grid h-7 w-7 cursor-pointer place-items-center rounded text-tx-3 transition-colors hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-accent"
    >
      <Ellipsis className="h-4 w-4" />
    </button>
  );

  return (
    <>
      <section
        ref={panelRef}
        className={cn(
          'group relative flex h-full min-h-0 flex-col overflow-hidden border border-bd-0 bg-bg-1',
          panel.transparent ? 'border-transparent bg-transparent' : 'rounded-md',
        )}
        aria-label={title}
      >
        <header className="flex h-9 shrink-0 items-center gap-2 border-b border-bd-0 px-2.5">
          <Tooltip>
            <TooltipTrigger asChild>
              <h3
                tabIndex={0}
                className="min-w-0 truncate font-sans text-xs font-semibold text-tx-1 focus-visible:bg-bg-3 focus-visible:text-tx-0"
              >
                {title}
              </h3>
            </TooltipTrigger>
            <TooltipContent side="top" className="max-w-sm break-words">
              {title}
            </TooltipContent>
          </Tooltip>
          {description && (
            <PanelDescriptionTooltip
              description={description}
              label={tr('Description')}
              panelTitle={title}
            />
          )}
          {data.state === 'streaming' && (
            <span
              role="status"
              aria-label={tr('Refreshing panel')}
              title={tr('Refreshing panel')}
              className="ml-auto grid h-5 w-5 place-items-center text-tx-3"
            >
              <RefreshCw aria-hidden="true" className="h-3 w-3 animate-spin" />
            </span>
          )}
          <span
            className={cn(
              'rounded-sm border border-bd-0 px-1.5 py-0.5 font-mono text-type-micro uppercase tracking-wide text-tx-3 opacity-100 transition-opacity group-hover:opacity-0 group-focus-within:opacity-0',
              data.state !== 'streaming' && 'ml-auto',
            )}
          >
            {tr(visualizationRegistry.get(panel.visualization.type).name)}
          </span>
          <div className="absolute right-1.5 opacity-0 transition-opacity group-hover:opacity-100 group-focus-within:opacity-100">
            <DropdownMenu>
              <DropdownMenuTrigger asChild>{menuButton}</DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="w-52">
                <DropdownMenuItem onSelect={() => setFullscreen(true)}>
                  <Fullscreen className="h-3.5 w-3.5" /> {tr('View')}
                </DropdownMenuItem>
                {!context.restricted && (
                  <>
                    {context.onEditPanel && (
                      <DropdownMenuItem
                        onSelect={() =>
                          context.onEditPanel?.(baseElementId(panel.id))
                        }
                      >
                        <Pencil className="h-3.5 w-3.5" /> {tr('Edit')}
                      </DropdownMenuItem>
                    )}
                    <DropdownMenuItem
                      onSelect={() =>
                        context.onDuplicatePanel
                          ? context.onDuplicatePanel(baseElementId(panel.id))
                          : copyText(
                              JSON.stringify(panel, null, 2),
                              tr('Panel JSON copied'),
                              tr('Could not copy panel JSON'),
                            )
                      }
                    >
                      <Copy className="h-3.5 w-3.5" /> {tr('Copy')}
                    </DropdownMenuItem>
                    {context.onRemovePanel && (
                      <DropdownMenuItem
                        className="text-danger"
                        onSelect={() =>
                          context.onRemovePanel?.(baseElementId(panel.id))
                        }
                      >
                        <Trash2 className="h-3.5 w-3.5" /> {tr('Remove')}
                      </DropdownMenuItem>
                    )}
                    <DropdownMenuSeparator />
                    <DropdownMenuItem
                      onSelect={() => {
                        const statement = activeQuery
                          ? queryExpression(activeQuery)
                          : '';
                        nav(
                          statement
                            ? `${exploreRoute}?query=${encodeURIComponent(interpolateVariables(statement, variables))}`
                            : exploreRoute,
                        );
                      }}
                    >
                      <Eye className="h-3.5 w-3.5" /> {tr('Explore data')}
                    </DropdownMenuItem>
                    <DropdownMenuItem
                      onSelect={() => setQueryInspector(true)}
                    >
                      <FileCode2 className="h-3.5 w-3.5" />{' '}
                      {tr('Inspect queries')}
                    </DropdownMenuItem>
                    <DropdownMenuItem
                      onSelect={() =>
                        sharePanel(
                          baseElementId(panel.id),
                          tr('Panel link copied'),
                          tr('Could not copy panel link'),
                        )
                      }
                    >
                      <Share2 className="h-3.5 w-3.5" /> {tr('Share')}
                    </DropdownMenuItem>
                    <DropdownMenuItem
                      onSelect={() => exportPanelData(title, data)}
                      disabled={data.frames.length === 0}
                    >
                      <Download className="h-3.5 w-3.5" />{' '}
                      {tr('Export data')}
                    </DropdownMenuItem>
                    <DropdownMenuItem
                      onSelect={() =>
                        exportPanelImage(
                          title,
                          panelRef.current,
                          tr('Could not export panel image'),
                        )
                      }
                    >
                      <ImageDown className="h-3.5 w-3.5" />{' '}
                      {tr('Export image')}
                    </DropdownMenuItem>
                    {panel.links.length > 0 && (
                      <DropdownMenuSub>
                        <DropdownMenuSubTrigger>
                          <ExternalLink className="h-3.5 w-3.5" />{' '}
                          {tr('Data links')}
                        </DropdownMenuSubTrigger>
                        <DropdownMenuSubContent>
                          {panel.links.map((link) => (
                            <DropdownMenuItem
                              key={link.id}
                              onSelect={() => {
                                const url = buildDataLinkUrl(link, {
                                  variables,
                                  timeRange: data.timeRange,
                                });
                                if (!url) return;
                                const external =
                                  link.target === 'external' ||
                                  /^https?:\/\//i.test(url);
                                if (link.openInNewTab) {
                                  window.open(url, '_blank', 'noopener');
                                } else if (external) {
                                  window.location.assign(url);
                                } else {
                                  nav(url);
                                }
                              }}
                            >
                              {link.title}
                            </DropdownMenuItem>
                          ))}
                        </DropdownMenuSubContent>
                      </DropdownMenuSub>
                    )}
                    {(context.canCreateAlert || context.canUseMoleAgent) && (
                      <DropdownMenuSeparator />
                    )}
                    {context.canCreateAlert && (
                      <DropdownMenuItem
                        onSelect={() =>
                          nav(
                            `/alerts/rules/new?dashboard=${encodeURIComponent(context.dashboard.uid)}&panel=${encodeURIComponent(baseElementId(panel.id))}`,
                          )
                        }
                      >
                        {tr('Create alert')}
                      </DropdownMenuItem>
                    )}
                    {context.canUseMoleAgent && (
                      <DropdownMenuItem
                        onSelect={() =>
                          nav(
                            `/intelligence/chat?dashboard=${encodeURIComponent(context.dashboard.uid)}&panel=${encodeURIComponent(baseElementId(panel.id))}`,
                          )
                        }
                      >
                        <Bot className="h-3.5 w-3.5" />{' '}
                        {tr('Mole Agent analysis')}
                      </DropdownMenuItem>
                    )}
                  </>
                )}
              </DropdownMenuContent>
            </DropdownMenu>
          </div>
        </header>
        <div className="min-h-0 flex-1 p-2">
          <VisualizationRenderer
            panel={renderPanel}
            data={data}
            height={Math.max(96, height - 52)}
            cursorScopeId={cursorScopeId}
          />
        </div>
      </section>
      <Dialog open={fullscreen} onOpenChange={setFullscreen}>
        <DialogContent className="h-[92vh] max-w-[96vw] grid-rows-[auto_minmax(0,1fr)]">
          <DialogHeader>
            <DialogTitle>{title}</DialogTitle>
          </DialogHeader>
          <div className="min-h-0 overflow-hidden border-t border-bd-0 pt-3">
            <VisualizationRenderer
              panel={renderPanel}
              data={data}
              height={Math.max(420, window.innerHeight * 0.78)}
              cursorScopeId={cursorScopeId}
            />
          </div>
        </DialogContent>
      </Dialog>
      {!context.restricted && (
        <Dialog open={queryInspector} onOpenChange={setQueryInspector}>
          <DialogContent className="max-w-3xl">
            <DialogHeader>
              <DialogTitle>
                {title} · {tr('Queries')}
              </DialogTitle>
            </DialogHeader>
            <pre className="max-h-[70vh] overflow-auto rounded-md border border-bd-0 bg-bg-0 p-4 font-mono text-xs text-tx-1">
              {JSON.stringify(panel.queries, null, 2)}
            </pre>
          </DialogContent>
        </Dialog>
      )}
    </>
  );
}

function DashboardTextCard({ element }: { element: DashboardTextElement }) {
  return (
    <section
      className={cn(
        'h-full overflow-auto p-3',
        !element.transparent && 'rounded-md border border-bd-0 bg-bg-1',
      )}
    >
      {element.title && (
        <h3 className="mb-2 font-sans text-xs font-semibold text-tx-1">
          {element.title}
        </h3>
      )}
      <div
        className={cn(
          'whitespace-pre-wrap text-sm leading-6 text-tx-1',
          element.mode === 'plain' ? 'font-mono text-xs' : 'font-sans',
        )}
      >
        {element.content}
      </div>
    </section>
  );
}

function DashboardContainerCard({
  element,
  variables,
  context,
  rowHeight,
  gap,
}: {
  element: DashboardGroup | DashboardRow;
  variables: DashboardVariableValues;
  context: DashboardRenderContext;
  rowHeight: number;
  gap: number;
}) {
  const tr = useDashboardText();
  const [collapsed, setCollapsed] = React.useState(element.collapsed ?? false);
  const children = React.useMemo(
    () =>
      expandRepeatedElements(
        element.elements,
        variables,
        context.dashboard.layout.columns,
      ),
    [context.dashboard.layout.columns, element.elements, variables],
  );
  return (
    <section className="flex h-full min-h-0 flex-col overflow-hidden rounded-md border border-bd-0 bg-bg-1">
      <button
        type="button"
        onClick={() => setCollapsed((value) => !value)}
        className="flex h-9 shrink-0 items-center gap-2 border-b border-bd-0 px-3 text-left font-sans text-xs font-semibold text-tx-1 hover:bg-bg-2"
      >
        {collapsed ? (
          <ChevronRight className="h-3.5 w-3.5" />
        ) : (
          <ChevronDown className="h-3.5 w-3.5" />
        )}
        {interpolateVariables(element.title, variables)}
        <span className="ml-auto font-mono text-type-micro uppercase text-tx-3">
          {tr(element.kind)}
        </span>
      </button>
      {!collapsed && (
        <div className="min-h-0 flex-1 overflow-auto p-2">
          <DashboardGrid
            elements={children}
            variables={variables}
            context={context}
            columns={context.dashboard.layout.columns}
            rowHeight={rowHeight}
            gap={gap}
            nested
          />
        </div>
      )}
    </section>
  );
}

function DashboardTabCard({
  element,
  variables,
  context,
  rowHeight,
  gap,
}: {
  element: DashboardTab;
  variables: DashboardVariableValues;
  context: DashboardRenderContext;
  rowHeight: number;
  gap: number;
}) {
  const defaultValue =
    element.defaultTabId ?? element.tabs[0]?.id ?? '';
  return (
    <section className="h-full min-h-0 overflow-hidden rounded-md border border-bd-0 bg-bg-1 p-2">
      <Tabs
        defaultValue={defaultValue}
        className="grid h-full min-h-0 grid-rows-[auto_minmax(0,1fr)]"
      >
        <TabsList className="h-8 w-fit justify-start">
          {element.tabs.map((tab) => (
            <TabsTrigger key={tab.id} value={tab.id} className="h-6 text-xs">
              {interpolateVariables(tab.title, variables)}
            </TabsTrigger>
          ))}
        </TabsList>
        {element.tabs.map((tab) => (
          <TabsContent
            key={tab.id}
            value={tab.id}
            className="min-h-0 overflow-auto"
          >
            <DashboardGrid
              elements={expandRepeatedElements(
                tab.elements,
                variables,
                context.dashboard.layout.columns,
              )}
              variables={variables}
              context={context}
              columns={context.dashboard.layout.columns}
              rowHeight={rowHeight}
              gap={gap}
              nested
            />
          </TabsContent>
        ))}
      </Tabs>
    </section>
  );
}

function DashboardVariableBar({
  dashboardUid,
  variables,
  values,
  timeRange,
  timeRangeKey,
  resolveTimeRange,
  refreshCadence,
  containerWidth,
  onChange,
  disabled,
  resolveQueries,
}: {
  dashboardUid: string;
  variables: readonly DashboardVariable[];
  values: DashboardVariableValues;
  timeRange: DashboardTimeRange;
  timeRangeKey: string;
  resolveTimeRange: DashboardTimeRangeResolver;
  refreshCadence: DashboardRefreshCadence;
  containerWidth: number;
  onChange: (values: DashboardVariableValues) => void;
  disabled: boolean;
  resolveQueries: boolean;
}) {
  const queryVariables = variables.filter(
    (variable) => variable.type === 'query',
  );
  const refreshInterval = resolveRefreshIntervalMilliseconds(
    refreshCadence,
    timeRange,
    containerWidth,
  );
  const queries = useQueries({
    queries: queryVariables.map((variable) => {
      const query = variable.query ?? {};
      const expression = interpolateVariables(
        stringValue(query.expression ?? query.query),
        values,
      );
      const kind = query.kind === 'sql' ? 'sql' : 'query';
      const streamName = stringValue(query.streamName ?? query.stream);
      const streamType = stringValue(query.streamType ?? query.stream_type);
      return {
        queryKey: [
          'dashboard-engine-variable',
          dashboardUid,
          variable.id,
          expression,
          timeRangeKey,
        ],
        queryFn: () => {
          const queryTimeRange = resolveTimeRange();
          return dashboardsApi.resolveVariable({
            variable: {
              name: variable.name,
              query: expression,
              kind,
              ...(streamName
                ? {
                    stream: {
                      name: streamName,
                      ...(streamType ? { stream_type: streamType } : {}),
                    },
                  }
                : {}),
            },
            time_range: {
              start: queryTimeRange.from,
              end: queryTimeRange.to,
            },
            limit: 500,
          });
        },
        enabled: resolveQueries && Boolean(expression),
        staleTime: 30_000,
        placeholderData: keepPreviousData,
        refetchInterval: refreshInterval,
      };
    }),
  });
  const resolved = new Map(
    queryVariables.map((variable, index) => [
      variable.id,
      queries[index]?.data?.values ?? [],
    ]),
  );
  const visible = variables.filter(
    (variable) =>
      variable.hide !== 'variable' &&
      variable.type !== 'constant',
  );
  if (visible.length === 0) return null;

  return (
    <div className="flex flex-wrap items-end gap-2 border-b border-bd-0 pb-3">
      {visible.map((variable) => {
        const resolvedValues = resolved.get(variable.id);
        const options =
          resolvedValues && resolvedValues.length > 0
            ? resolvedValues.map((value) => ({ label: value, value }))
            : variableOptions(variable);
        return (
          <VariableControl
            key={variable.id}
            variable={variable}
            options={options}
            value={values[variable.name]}
            disabled={disabled}
            onChange={(value) =>
              onChange({ ...values, [variable.name]: value })
            }
          />
        );
      })}
    </div>
  );
}

function VariableControl({
  variable,
  options,
  value,
  onChange,
  disabled,
}: {
  variable: DashboardVariable;
  options: Array<{ label: string; value: unknown }>;
  value: unknown;
  onChange: (value: unknown) => void;
  disabled: boolean;
}) {
  const tr = useDashboardText();
  if (variable.type === 'text') {
    return (
      <label className="grid gap-1 font-sans text-type-micro text-tx-3">
        {variable.hide === 'label' ? null : variable.label}
        <input
          disabled={disabled}
          value={String(value ?? '')}
          onChange={(event) => onChange(event.target.value)}
          className="h-8 min-w-36 rounded-md border border-bd-1 bg-bg-1 px-2 font-mono text-xs text-tx-1 outline-none focus-visible:bg-bg-2 disabled:cursor-not-allowed disabled:opacity-60"
        />
      </label>
    );
  }
  if (variable.multi) {
    const selected = Array.isArray(value) ? value : value === undefined ? [] : [value];
    return (
      <div className="grid gap-1 font-sans text-type-micro text-tx-3">
        {variable.hide === 'label' ? null : variable.label}
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <ChromeButton
              disabled={disabled}
              className="min-w-36 justify-between"
            >
              <span className="max-w-48 truncate">
                {selected.length === 0
                  ? tr('Select')
                  : selected.map(String).join(', ')}
              </span>
              <ChevronDown className="h-3 w-3" />
            </ChromeButton>
          </DropdownMenuTrigger>
          <DropdownMenuContent className="max-h-72 min-w-56 overflow-auto">
            <DropdownMenuLabel>{variable.label}</DropdownMenuLabel>
            {options.map((option) => {
              const checked = selected.some((entry) =>
                Object.is(entry, option.value),
              );
              return (
                <DropdownMenuCheckboxItem
                  key={`${option.label}-${String(option.value)}`}
                  checked={checked}
                  disabled={disabled}
                  onSelect={(event) => event.preventDefault()}
                  onCheckedChange={(next) =>
                    onChange(
                      next
                        ? [...selected, option.value]
                        : selected.filter(
                            (entry) => !Object.is(entry, option.value),
                          ),
                    )
                  }
                >
                  {option.label}
                </DropdownMenuCheckboxItem>
              );
            })}
          </DropdownMenuContent>
        </DropdownMenu>
      </div>
    );
  }
  return (
    <label className="grid gap-1 font-sans text-type-micro text-tx-3">
      {variable.hide === 'label' ? null : variable.label}
      <select
        disabled={disabled}
        value={String(value ?? '')}
        onChange={(event) => {
          const option = options.find(
            (candidate) => String(candidate.value) === event.target.value,
          );
          onChange(option?.value ?? event.target.value);
        }}
        className="h-8 min-w-36 rounded-md border border-bd-1 bg-bg-1 px-2 font-sans text-xs text-tx-1 outline-none focus-visible:bg-bg-2 disabled:cursor-not-allowed disabled:opacity-60"
      >
        {options.map((option) => (
          <option
            key={`${option.label}-${String(option.value)}`}
            value={String(option.value)}
          >
            {option.label}
          </option>
        ))}
      </select>
    </label>
  );
}

function signalExploreRoute(type: PanelQuery['dataSourceType']): string {
  if (type === 'logs') return '/logs';
  if (type === 'traces') return '/traces';
  if (type === 'profiles') return '/profiles';
  return '/metrics';
}

function queryExpression(query: PanelQuery): string {
  for (const key of ['expression', 'statement', 'sql', 'query']) {
    const value = query.query[key];
    if (typeof value === 'string' && value.trim()) return value;
  }
  return '';
}

function exportPanelData(title: string, data: PanelData): void {
  const rows = data.frames.flatMap((frame) => {
    const header = frame.fields.map((field) => field.name);
    const body = Array.from({ length: frame.length }, (_, index) =>
      frame.fields.map((field) => field.values[index]),
    );
    return [header, ...body];
  });
  const csv = rows.map((row) => row.map(csvCell).join(',')).join('\n');
  const blob = new Blob([csv], { type: 'text/csv;charset=utf-8' });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement('a');
  anchor.href = url;
  anchor.download = `${safeFilename(title)}.csv`;
  anchor.click();
  URL.revokeObjectURL(url);
}

function exportPanelImage(
  title: string,
  panel: HTMLElement | null,
  errorMessage: string,
): void {
  if (!panel) {
    toast.error(errorMessage);
    return;
  }
  const clone = panel.cloneNode(true) as HTMLElement;
  const sourceNodes = [panel, ...panel.querySelectorAll<HTMLElement>('*')];
  const cloneNodes = [clone, ...clone.querySelectorAll<HTMLElement>('*')];
  sourceNodes.forEach((source, index) => {
    const target = cloneNodes[index];
    if (!target) return;
    const computed = window.getComputedStyle(source);
    target.setAttribute(
      'style',
      Array.from(computed)
        .map(
          (property) =>
            `${property}:${computed.getPropertyValue(property)};`,
        )
        .join(''),
    );
  });
  const sourceCanvases = panel.querySelectorAll('canvas');
  const cloneCanvases = clone.querySelectorAll('canvas');
  sourceCanvases.forEach((canvas, index) => {
    const replacement = document.createElement('img');
    replacement.src = canvas.toDataURL('image/png');
    replacement.width = canvas.clientWidth;
    replacement.height = canvas.clientHeight;
    cloneCanvases[index]?.replaceWith(replacement);
  });
  const bounds = panel.getBoundingClientRect();
  const serialized = new XMLSerializer().serializeToString(clone);
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${Math.ceil(bounds.width)}" height="${Math.ceil(bounds.height)}" viewBox="0 0 ${Math.ceil(bounds.width)} ${Math.ceil(bounds.height)}"><foreignObject width="100%" height="100%"><div xmlns="http://www.w3.org/1999/xhtml">${serialized}</div></foreignObject></svg>`;
  const blob = new Blob([svg], { type: 'image/svg+xml;charset=utf-8' });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement('a');
  anchor.href = url;
  anchor.download = `${safeFilename(title)}.svg`;
  anchor.click();
  URL.revokeObjectURL(url);
}

function sharePanel(
  panelId: string,
  successMessage: string,
  errorMessage: string,
): void {
  const url = new URL(window.location.href);
  url.searchParams.set('viewPanel', panelId);
  void navigator.clipboard
    .writeText(url.toString())
    .then(() => toast.success(successMessage))
    .catch(() => toast.error(errorMessage));
}

function copyText(
  value: string,
  successMessage: string,
  errorMessage: string,
): void {
  void navigator.clipboard
    .writeText(value)
    .then(() => toast.success(successMessage))
    .catch(() => toast.error(errorMessage));
}

function csvCell(value: unknown): string {
  const text =
    typeof value === 'object' ? JSON.stringify(value) : String(value ?? '');
  return `"${text.replaceAll('"', '""')}"`;
}

function safeFilename(value: string): string {
  return value.replace(/[^\p{L}\p{N}._-]+/gu, '-') || 'panel-data';
}

function baseElementId(id: string): string {
  return id.split('::repeat::')[0] ?? id;
}

function dashboardLinkUrl(
  rawUrl: string,
  options: {
    variables: DashboardVariableValues;
    timeRange: DashboardTimeRange;
    includeTimeRange: boolean;
    includeVariables: boolean;
  },
): string {
  const interpolated = interpolateVariables(rawUrl, options.variables);
  const origin =
    typeof window === 'undefined' ? 'http://dashboard.local' : window.location.origin;
  const url = new URL(interpolated || '/', origin);
  if (options.includeTimeRange) {
    url.searchParams.set('from', String(options.timeRange.from));
    url.searchParams.set('to', String(options.timeRange.to));
  }
  if (options.includeVariables) {
    for (const [name, value] of Object.entries(options.variables)) {
      url.searchParams.set(
        `var-${name}`,
        Array.isArray(value) ? value.join(',') : String(value ?? ''),
      );
    }
  }
  return url.origin === origin
    ? `${url.pathname}${url.search}${url.hash}`
    : url.toString();
}

function stringValue(value: unknown): string {
  return typeof value === 'string' ? value : '';
}

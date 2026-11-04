import { ChevronDown, ChevronUp, CircleAlert, ChevronsDownUp, ChevronsUpDown, PanelRightOpen, Route, Search, X } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';
import { ViewTabs } from '@/shell/tabs/ViewTabs';
import { Button } from '@/shell/ui/button';
import { Input } from '@/shell/ui/input';
import { Tooltip, TooltipContent, TooltipTrigger } from '@/shell/ui/tooltip';

import { formatTraceDurationNs } from '../duration';
import { TRACE_VIEW_KINDS, type TraceViewState } from './state';
import type { TraceTree } from '../model/tree';
import { TraceOverview } from '../waterfall/Overview';

function IconToggle({
  label,
  pressed,
  onClick,
  children,
  disabled,
}: {
  label: string;
  pressed?: boolean | undefined;
  onClick: () => void;
  children: React.ReactNode;
  disabled?: boolean | undefined;
}) {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          variant="ghost"
          size="icon"
          aria-label={label}
          {...(pressed !== undefined ? { 'aria-pressed': pressed } : {})}
          disabled={disabled}
          onClick={onClick}
          className={cn('h-7 w-7 shrink-0', pressed && 'bg-indigo-dim text-indigo-soft')}
        >
          {children}
        </Button>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}

/**
 * The bar over the trace's views: which view, and what narrows it — a search
 * that steps from match to match, failed spans only, folding, the span the
 * view is narrowed to and the stretch of time it is zoomed to.
 */
export function TraceToolbar({
  tree,
  state,
  panelOpen,
  onOpenPanel,
}: {
  tree: TraceTree;
  state: TraceViewState;
  panelOpen: boolean;
  onOpenPanel: () => void;
}) {
  const { t } = useTranslation('traces');
  const focus = state.focusId ? tree.byId.get(state.focusId) : undefined;
  const lined = state.view === 'waterfall' || state.view === 'flame';

  return (
    <div className="flex shrink-0 flex-wrap items-center gap-2 border-b border-bd-0 px-3 py-1.5">
      <ViewTabs
        label={t('view.views.label')}
        value={state.view}
        onValueChange={state.setView}
        items={TRACE_VIEW_KINDS.map((kind) => ({ key: kind, label: t(`view.views.${kind}`) }))}
      />

      {lined && (
        <div className="ml-auto flex min-w-0 flex-wrap items-center justify-end gap-1.5">
          {focus && (
            <Chip
              label={t('view.toolbar.focused', { operation: focus.span.operation })}
              clearLabel={t('view.toolbar.unfocus')}
              onClear={() => state.focus(null)}
            />
          )}
          {state.zoom && (
            <Chip
              label={t('view.toolbar.zoomed', {
                from: formatTraceDurationNs(state.zoom.fromNs),
                to: formatTraceDurationNs(state.zoom.toNs),
              })}
              clearLabel={t('view.toolbar.unzoom')}
              onClear={() => state.setZoom(null)}
            />
          )}
          <div className="relative w-[220px] min-w-[160px]">
            <Search aria-hidden="true" className="pointer-events-none absolute left-2 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-tx-3" />
            <Input
              value={state.search}
              onChange={(event) => state.setSearch(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === 'Enter') {
                  event.preventDefault();
                  state.stepMatch(event.shiftKey ? -1 : 1);
                }
              }}
              aria-label={t('view.toolbar.search')}
              placeholder={t('view.toolbar.search_placeholder')}
              className="h-7 pl-7 pr-14 font-sans text-xs"
              data-testid="trace-span-search"
            />
            {state.search.trim() && (
              <span aria-live="polite" className="pointer-events-none absolute right-2 top-1/2 -translate-y-1/2 font-sans text-xs tabular-nums text-tx-3">
                {state.matches.length === 0 ? t('view.toolbar.no_match') : `${Math.max(0, state.matchIndex) + 1}/${state.matches.length}`}
              </span>
            )}
          </div>
          <IconToggle label={t('view.toolbar.previous_match')} onClick={() => state.stepMatch(-1)} disabled={state.matches.length === 0}>
            <ChevronUp className="h-3.5 w-3.5" />
          </IconToggle>
          <IconToggle label={t('view.toolbar.next_match')} onClick={() => state.stepMatch(1)} disabled={state.matches.length === 0}>
            <ChevronDown className="h-3.5 w-3.5" />
          </IconToggle>
          <IconToggle
            label={t('view.toolbar.errors_only', { count: tree.errorCount })}
            pressed={state.errorsOnly}
            onClick={() => state.setErrorsOnly(!state.errorsOnly)}
            disabled={tree.errorCount === 0 && !state.errorsOnly}
          >
            <CircleAlert className="h-3.5 w-3.5" />
          </IconToggle>
          <IconToggle label={t('view.toolbar.critical_path')} pressed={state.showCritical} onClick={() => state.setShowCritical(!state.showCritical)}>
            <Route className="h-3.5 w-3.5" />
          </IconToggle>
          {state.view === 'waterfall' && (
            <>
              <IconToggle label={t('view.toolbar.expand_all')} onClick={state.expandAll}>
                <ChevronsUpDown className="h-3.5 w-3.5" />
              </IconToggle>
              <IconToggle label={t('view.toolbar.collapse_all')} onClick={state.collapseAll}>
                <ChevronsDownUp className="h-3.5 w-3.5" />
              </IconToggle>
            </>
          )}
        </div>
      )}

      {!panelOpen && (
        <div className={cn(!lined && 'ml-auto')}>
          <IconToggle label={t('view.toolbar.open_panel')} onClick={onOpenPanel}>
            <PanelRightOpen className="h-3.5 w-3.5" />
          </IconToggle>
        </div>
      )}
    </div>
  );
}

function Chip({ label, clearLabel, onClear }: { label: string; clearLabel: string; onClear: () => void }) {
  return (
    <span className="inline-flex h-7 min-w-0 max-w-[260px] items-center gap-1 rounded-md bg-[var(--indigo-tint)] pl-2 pr-0.5 font-sans text-xs text-indigo">
      <span className="min-w-0 truncate">{label}</span>
      <button
        type="button"
        aria-label={clearLabel}
        title={clearLabel}
        onClick={onClear}
        className="grid h-6 w-6 shrink-0 place-items-center rounded hover:bg-indigo/15 focus-visible:bg-indigo/15 focus-visible:outline-none"
      >
        <X className="h-3 w-3" />
      </button>
    </span>
  );
}

/** The services of the trace in their colours, with their part of its work; and the whole trace at a glance beside them. */
export function TraceLegendRow({
  tree,
  state,
  colors,
  namesWidth,
}: {
  tree: TraceTree;
  state: TraceViewState;
  colors: ReadonlyMap<string, string>;
  namesWidth: number;
}) {
  const { t } = useTranslation('traces');
  const { critical } = state;
  // With the critical path drawn, a service's part is its time on the path, not its part of all the work.
  const services = critical
    ? critical.services.map((service) => ({
        ...service,
        title: t('view.legend.critical_item', {
          service: service.name,
          percent: Math.round(service.share * 1000) / 10,
          duration: formatTraceDurationNs(service.ns),
        }),
      }))
    : tree.services.map((service) => ({
        ...service,
        title: t('view.legend.item', { service: service.name, percent: Math.round(service.share * 1000) / 10 }),
      }));
  const shown = services.slice(0, 6);
  const more = services.length - shown.length;

  return (
    <div
      className="grid shrink-0 grid-cols-[var(--legend-width)_minmax(0,1fr)] items-center gap-3 border-b border-bd-0 py-1.5 pl-3 pr-[calc(0.75rem+var(--scrollbar-gutter,0px))]"
      style={{ '--legend-width': `${Math.max(160, namesWidth - 12)}px` } as React.CSSProperties}
    >
      <ul
        aria-label={t(critical ? 'view.legend.critical_aria' : 'view.legend.aria')}
        className="flex min-w-0 flex-wrap items-center gap-x-2.5 gap-y-0.5"
        data-testid="trace-legend"
        data-legend={critical ? 'critical' : 'work'}
      >
        {critical && (
          <li className="flex shrink-0 items-center gap-1 font-sans text-xs font-strong text-tx-1" data-testid="trace-critical-total">
            <Route aria-hidden="true" className="h-3 w-3 text-tx-2" />
            {t('view.legend.critical', { duration: formatTraceDurationNs(critical.totalNs) })}
          </li>
        )}
        {shown.map((service) => (
          <li key={service.name} className="flex min-w-0 items-center gap-1 font-sans text-xs" title={service.title} data-legend-service={service.name}>
            <span aria-hidden="true" className="h-2 w-2 shrink-0 rounded-sm" style={{ backgroundColor: service.color }} />
            <span className="min-w-0 max-w-[120px] truncate text-tx-1">{service.name}</span>
            <span className="tabular-nums text-tx-3">{Math.round(service.share * 100)}%</span>
          </li>
        ))}
        {more > 0 && <li className="font-sans text-xs text-tx-3">{t('view.legend.more', { count: more })}</li>}
      </ul>
      <TraceOverview tree={tree} colors={colors} zoom={state.zoom} onZoom={state.setZoom} critical={critical?.spans ?? null} />
    </div>
  );
}

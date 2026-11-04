import * as React from 'react';

import { useTraceViewPrefs } from './prefs';
import { type CriticalPath, criticalPath } from '../model/criticalPath';
import { foldableIds, foldedAbove, waterfallRows, type WaterfallRow } from '../model/rows';
import { searchSpans } from '../model/search';
import type { TraceTree } from '../model/tree';

/** The ways a trace is drawn. */
export type TraceViewKind = 'waterfall' | 'flame' | 'spans' | 'map';
export const TRACE_VIEW_KINDS: readonly TraceViewKind[] = ['waterfall', 'flame', 'spans', 'map'];

/** A stretch of the trace the time axis is narrowed to, as offsets from its start. */
export interface TraceZoom {
  fromNs: number;
  toNs: number;
}

export interface TraceViewState {
  view: TraceViewKind;
  setView: (view: TraceViewKind) => void;
  selectedId: string | null;
  /** Selects a span, opening whatever is folded above it, and leaving a narrowed view it is outside of. */
  select: (id: string | null) => void;
  collapsed: ReadonlySet<string>;
  toggle: (id: string) => void;
  collapseAll: () => void;
  expandAll: () => void;
  focusId: string | null;
  focus: (id: string | null) => void;
  errorsOnly: boolean;
  setErrorsOnly: (errorsOnly: boolean) => void;
  search: string;
  setSearch: (search: string) => void;
  /** The spans the search found, in the order the waterfall lists them. */
  matches: readonly string[];
  matchSet: ReadonlySet<string> | null;
  /** Which match is the current one; -1 without any. */
  matchIndex: number;
  /** Moves to the next match, or the previous one, selecting it. */
  stepMatch: (step: 1 | -1) => void;
  zoom: TraceZoom | null;
  setZoom: (zoom: TraceZoom | null) => void;
  /** The critical path is drawn. */
  showCritical: boolean;
  setShowCritical: (show: boolean) => void;
  /** The trace's critical path, worked out only while it is drawn. */
  critical: CriticalPath | null;
  /** The lines of the waterfall as things stand. */
  rows: WaterfallRow[];
}

/**
 * What the reader did to a trace's view: the span picked, what is folded or
 * narrowed, the search and the stretch of time in view. A trace opened anew
 * begins on the span asked for, or on its root.
 */
export function useTraceViewState(tree: TraceTree | null, initialSpanId?: string | null): TraceViewState {
  const [view, setView] = React.useState<TraceViewKind>('waterfall');
  const [selectedId, setSelectedId] = React.useState<string | null>(null);
  const [collapsed, setCollapsed] = React.useState<ReadonlySet<string>>(() => new Set());
  const [focusId, setFocusId] = React.useState<string | null>(null);
  const [errorsOnly, setErrorsOnly] = React.useState(false);
  const [search, setSearch] = React.useState('');
  const [matchIndex, setMatchIndex] = React.useState(-1);
  const [zoom, setZoom] = React.useState<TraceZoom | null>(null);
  const showCritical = useTraceViewPrefs((prefs) => prefs.criticalPath);
  const setShowCritical = useTraceViewPrefs((prefs) => prefs.setCriticalPath);
  const critical = React.useMemo(() => (tree && showCritical ? criticalPath(tree) : null), [showCritical, tree]);

  const traceId = tree?.traceId ?? null;

  // A trace opened anew: what was done to the last one does not carry over.
  React.useEffect(() => {
    if (!tree) return;
    const asked = initialSpanId && tree.byId.has(initialSpanId) ? initialSpanId : null;
    setSelectedId(asked ?? tree.root.id);
    setCollapsed(new Set());
    setFocusId(null);
    setZoom(null);
    setMatchIndex(-1);
    // Only a new trace, or a new span asked for, starts over.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [traceId, initialSpanId]);

  const matchSet = React.useMemo(() => (tree && search.trim() ? searchSpans(tree, search) : null), [search, tree]);
  const matches = React.useMemo(
    () => (tree && matchSet ? tree.nodes.filter((node) => matchSet.has(node.id)).map((node) => node.id) : []),
    [matchSet, tree],
  );

  React.useEffect(() => setMatchIndex(-1), [search]);

  const rows = React.useMemo(
    () => (tree ? waterfallRows(tree, { collapsed, focusId, errorsOnly, matches: matchSet }) : []),
    [collapsed, errorsOnly, focusId, matchSet, tree],
  );

  const select = React.useCallback((id: string | null) => {
    setSelectedId(id);
    if (!tree || !id) return;

    const folded = foldedAbove(tree, id, collapsed);
    if (folded.length > 0) {
      setCollapsed((current) => new Set([...current].filter((other) => !folded.includes(other))));
    }

    // A span outside the narrowed part of the trace is not one that can be shown in it.
    if (focusId && focusId !== id && !isUnder(tree, id, focusId)) setFocusId(null);
  }, [collapsed, focusId, tree]);

  const toggle = React.useCallback((id: string) => {
    setCollapsed((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const collapseAll = React.useCallback(() => {
    if (!tree) return;
    // The root stays open: a trace folded into one line says nothing.
    const foldable = foldableIds(tree);
    for (const root of tree.roots) foldable.delete(root.id);
    setCollapsed(foldable);
  }, [tree]);

  const expandAll = React.useCallback(() => setCollapsed(new Set()), []);

  const focus = React.useCallback((id: string | null) => {
    setFocusId(id);
    setZoom(null);
    if (id) setSelectedId(id);
  }, []);

  const stepMatch = React.useCallback((step: 1 | -1) => {
    if (matches.length === 0) return;
    const next = (matchIndex + step + matches.length) % matches.length;
    setMatchIndex(next);
    select(matches[next]!);
  }, [matchIndex, matches, select]);

  return {
    view,
    setView,
    selectedId,
    select,
    collapsed,
    toggle,
    collapseAll,
    expandAll,
    focusId,
    focus,
    errorsOnly,
    setErrorsOnly,
    search,
    setSearch,
    matches,
    matchSet,
    matchIndex,
    stepMatch,
    zoom,
    setZoom,
    showCritical,
    setShowCritical,
    critical,
    rows,
  };
}

function isUnder(tree: TraceTree, id: string, ancestorId: string): boolean {
  const seen = new Set<string>();
  let node = tree.byId.get(id);

  while (node?.parentId && !seen.has(node.parentId)) {
    if (node.parentId === ancestorId) return true;
    seen.add(node.parentId);
    node = tree.byId.get(node.parentId);
  }

  return false;
}

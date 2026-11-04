import type { TraceNode, TraceTree } from './tree';

/** A line of the waterfall: a span, how deep it stands, and what the reader did to it. */
export interface WaterfallRow {
  node: TraceNode;
  /** Its depth under the span the view begins at. */
  depth: number;
  hasChildren: boolean;
  collapsed: boolean;
  /** The search found it. */
  matched: boolean;
  /** Shown only as the way from the top to a span that is kept: an error, or a match. */
  path: boolean;
}

export interface WaterfallRowOptions {
  /** The spans whose children are folded away. */
  collapsed: ReadonlySet<string>;
  /** The span the view is narrowed to, with what lies under it; the whole trace without one. */
  focusId?: string | null | undefined;
  /** Keep failed spans and the way down to them. */
  errorsOnly?: boolean | undefined;
  /** The spans a search found. */
  matches?: ReadonlySet<string> | null | undefined;
  /** Keep only what the search found, and the way down to it. */
  matchesOnly?: boolean | undefined;
}

/**
 * The lines of the waterfall, each span after its parent. Narrowing to errors
 * or matches keeps the way from the top down to each span kept, as a path of
 * its own, so a span never stands without its callers.
 */
export function waterfallRows(tree: TraceTree, options: WaterfallRowOptions): WaterfallRow[] {
  const focus = options.focusId ? tree.byId.get(options.focusId) : undefined;
  const starts = focus ? [focus] : tree.roots;
  const baseDepth = focus?.depth ?? 0;
  const matches = options.matches ?? null;

  const wanted = (node: TraceNode) =>
    (!options.errorsOnly || node.failed) && (!options.matchesOnly || matches === null || matches.has(node.id));

  const narrowing = Boolean(options.errorsOnly || (options.matchesOnly && matches));
  const kept = narrowing ? keptWithPaths(tree, starts, wanted) : null;
  const rows: WaterfallRow[] = [];
  const stack = [...starts].reverse();

  while (stack.length > 0) {
    const node = stack.pop()!;
    if (kept && !kept.has(node.id)) continue;

    const children = kept ? node.children.filter((child) => kept.has(child.id)) : node.children;
    const collapsed = options.collapsed.has(node.id);

    rows.push({
      node,
      depth: Math.max(0, node.depth - baseDepth),
      hasChildren: children.length > 0,
      collapsed: collapsed && children.length > 0,
      matched: matches?.has(node.id) ?? false,
      path: kept !== null && !wanted(node),
    });

    if (collapsed) continue;
    for (let index = children.length - 1; index >= 0; index -= 1) stack.push(children[index]!);
  }

  return rows;
}

/** The spans under `starts` that `wanted` keeps, with every span on the way down to one. */
function keptWithPaths(tree: TraceTree, starts: readonly TraceNode[], wanted: (node: TraceNode) => boolean): Set<string> {
  const kept = new Set<string>();
  const inside = new Set<string>();
  const stack = [...starts];

  while (stack.length > 0) {
    const node = stack.pop()!;
    inside.add(node.id);
    stack.push(...node.children);
  }

  for (const id of inside) {
    let node = tree.byId.get(id);
    if (!node || !wanted(node)) continue;

    // Up to the first span already kept, or out of the part of the tree in view.
    while (node && inside.has(node.id) && !kept.has(node.id)) {
      kept.add(node.id);
      node = node.parentId ? tree.byId.get(node.parentId) : undefined;
    }
  }

  return kept;
}

/** Every span that has spans under it: what "fold all" folds. */
export function foldableIds(tree: TraceTree): Set<string> {
  return new Set(tree.nodes.filter((node) => node.children.length > 0).map((node) => node.id));
}

/** The spans folded away above `id`: what has to open for it to be seen. */
export function foldedAbove(tree: TraceTree, id: string, collapsed: ReadonlySet<string>): string[] {
  const folded: string[] = [];
  const seen = new Set<string>();
  let node = tree.byId.get(id);
  let parentId = node?.parentId ?? null;

  while (parentId && !seen.has(parentId)) {
    seen.add(parentId);
    if (collapsed.has(parentId)) folded.push(parentId);
    node = tree.byId.get(parentId);
    parentId = node?.parentId ?? null;
  }

  return folded;
}

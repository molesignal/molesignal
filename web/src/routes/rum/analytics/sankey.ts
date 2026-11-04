import { PATHS_OTHER } from '@/api/rum';
import type { PathLink, PathNode } from '@/api/rum';

/** What is drawn of a column of pages. */
export interface SankeyNode {
  key: string;
  column: number;
  page: string;
  sessions: number;
  ends: number;
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface SankeyLink {
  key: string;
  source: SankeyNode;
  target: SankeyNode;
  sessions: number;
  /** The path of the ribbon, a curve between the two edges, drawn with a stroke as thick as `thickness`. */
  d: string;
  thickness: number;
}

export interface SankeyLayout {
  width: number;
  height: number;
  nodes: SankeyNode[];
  links: SankeyLink[];
}

export interface SankeyBox {
  width: number;
  height: number;
  nodeWidth: number;
  gap: number;
  /** Room left of the first column and right of the last one, for the names. */
  margin: number;
  /** The most height a session is given, so that a few of them do not fill the box. */
  maxPerSession?: number | undefined;
}

/**
 * The columns that have pages in them, numbered from 0: an answer has as many columns as pages were
 * asked for, and the ones nobody got to (after the last page of the longest way, or before the
 * first when going backward) are not drawn.
 */
export function usedColumns(
  nodes: readonly PathNode[],
  links: readonly PathLink[],
): { nodes: PathNode[]; links: PathLink[]; columns: number } {
  if (nodes.length === 0) return { nodes: [], links: [], columns: 1 };

  const first = Math.min(...nodes.map((node) => node.column));
  const last = Math.max(...nodes.map((node) => node.column));

  return {
    nodes: nodes.map((node) => ({ ...node, column: node.column - first })),
    links: links.map((link) => ({ ...link, column: link.column - first })),
    columns: last - first + 1,
  };
}

export const nodeKey = (column: number, page: string): string => `${column}\u0000${page}`;

/**
 * Lays columns of pages out left to right: the pages of a column stacked, tallest first, as tall as
 * the sessions that were on them, every column scaled by the busiest, and a ribbon from each page to
 * the pages of the next column, as thick as the sessions that went that way. Ribbons leave and enter
 * a page in the order of the pages they join, so they do not cross within a page.
 */
export function layoutSankey(nodes: readonly PathNode[], links: readonly PathLink[], columns: number, box: SankeyBox): SankeyLayout {
  const usable = Math.max(1, box.width - 2 * box.margin - box.nodeWidth);
  const step = columns > 1 ? usable / (columns - 1) : 0;

  const byColumn = Array.from({ length: columns }, (_, column) =>
    nodes
      .filter((node) => node.column === column)
      .sort((a, b) => Number(a.page === PATHS_OTHER) - Number(b.page === PATHS_OTHER) || b.sessions - a.sessions || a.page.localeCompare(b.page)),
  );

  // The scale that fits the column with the most in it, its gaps included, into the height.
  const fits = byColumn
    .filter((column) => column.length > 0)
    .map((column) => {
      const total = column.reduce((sum, node) => sum + node.sessions, 0);

      return total > 0 ? (box.height - (column.length - 1) * box.gap) / total : Number.POSITIVE_INFINITY;
    });
  const fitted = fits.length > 0 && Math.min(...fits) !== Number.POSITIVE_INFINITY ? Math.max(0, Math.min(...fits)) : 0;
  const scale = Math.min(fitted, box.maxPerSession ?? Number.POSITIVE_INFINITY);

  const placed = new Map<string, SankeyNode>();

  byColumn.forEach((column, index) => {
    let y = 0;

    for (const node of column) {
      const height = Math.max(2, node.sessions * scale);

      placed.set(nodeKey(index, node.page), {
        key: nodeKey(index, node.page),
        column: index,
        page: node.page,
        sessions: node.sessions,
        ends: node.ends,
        x: box.margin + index * step,
        y,
        width: box.nodeWidth,
        height,
      });

      y += height + box.gap;
    }
  });

  // Where the next ribbon leaves a page (downwards) and enters one.
  const out = new Map<string, number>();
  const into = new Map<string, number>();

  const order = [...links].sort(
    (a, b) => a.column - b.column || (placed.get(nodeKey(a.column, a.source))?.y ?? 0) - (placed.get(nodeKey(b.column, b.source))?.y ?? 0) || (placed.get(nodeKey(a.column + 1, a.target))?.y ?? 0) - (placed.get(nodeKey(b.column + 1, b.target))?.y ?? 0),
  );

  const ribbons = order.flatMap((link): SankeyLink[] => {
    const source = placed.get(nodeKey(link.column, link.source));
    const target = placed.get(nodeKey(link.column + 1, link.target));

    if (!source || !target) return [];

    const thickness = Math.max(1, link.sessions * scale);
    const from = out.get(source.key) ?? 0;
    const to = into.get(target.key) ?? 0;

    out.set(source.key, from + thickness);
    into.set(target.key, to + thickness);

    const x0 = source.x + source.width;
    const x1 = target.x;
    const y0 = source.y + from + thickness / 2;
    const y1 = target.y + to + thickness / 2;
    const middle = (x0 + x1) / 2;

    return [
      {
        key: `${source.key}>${target.key}`,
        source,
        target,
        sessions: link.sessions,
        d: `M ${x0} ${y0} C ${middle} ${y0}, ${middle} ${y1}, ${x1} ${y1}`,
        thickness,
      },
    ];
  });

  const height = Math.max(0, ...[...placed.values()].map((node) => node.y + node.height));

  return { width: box.width, height, nodes: [...placed.values()], links: ribbons };
}

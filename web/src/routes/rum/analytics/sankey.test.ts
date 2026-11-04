import { describe, expect, it } from 'vitest';

import { PATHS_OTHER } from '@/api/rum';
import type { PathLink, PathNode } from '@/api/rum';

import { layoutSankey, nodeKey, usedColumns } from './sankey';

const BOX = { width: 600, height: 300, nodeWidth: 10, gap: 10, margin: 20 };

const nodes: PathNode[] = [
  { column: 0, page: '/', sessions: 100, ends: 0 },
  { column: 1, page: '/products', sessions: 70, ends: 20 },
  { column: 1, page: '/checkout', sessions: 30, ends: 30 },
  { column: 2, page: '/checkout', sessions: 50, ends: 50 },
];

const links: PathLink[] = [
  { column: 0, source: '/', target: '/products', sessions: 70 },
  { column: 0, source: '/', target: '/checkout', sessions: 30 },
  { column: 1, source: '/products', target: '/checkout', sessions: 50 },
];

describe('the layout of paths', () => {
  const layout = layoutSankey(nodes, links, 3, BOX);
  const node = (column: number, page: string) => layout.nodes.find((candidate) => candidate.key === nodeKey(column, page))!;

  it('stacks the pages of a column, the busiest first, as tall as their sessions', () => {
    const products = node(1, '/products');
    const checkout = node(1, '/checkout');

    expect(products.y).toBe(0);
    expect(checkout.y).toBeCloseTo(products.height + BOX.gap);
    expect(products.height / checkout.height).toBeCloseTo(70 / 30);
  });

  it('puts the pages that were pooled last, though they hold more sessions than any page', () => {
    const pooled = layoutSankey(
      [
        { column: 0, page: '/a', sessions: 2, ends: 2 },
        { column: 0, page: PATHS_OTHER, sessions: 9, ends: 9 },
        { column: 0, page: '/b', sessions: 1, ends: 1 },
      ],
      [],
      1,
      BOX,
    );
    const at = (page: string) => pooled.nodes.find((candidate) => candidate.page === page)!.y;

    expect(at('/a')).toBeLessThan(at('/b'));
    expect(at('/b')).toBeLessThan(at(PATHS_OTHER));
  });

  it('scales every column by the one that needs the most room, so the fullest fits the height', () => {
    const first = node(0, '/');
    const second = node(1, '/products').height + node(1, '/checkout').height + BOX.gap;

    expect(first.y + first.height).toBeLessThanOrEqual(BOX.height + 0.001);
    expect(second).toBeLessThanOrEqual(BOX.height + 0.001);
    // 100 sessions in one page against 100 in two with a gap: the second column decides.
    expect(first.height).toBeCloseTo((BOX.height - BOX.gap) );
  });

  it('puts the columns at equal distances between the margins', () => {
    expect(node(0, '/').x).toBe(BOX.margin);
    expect(node(2, '/checkout').x).toBe(BOX.width - BOX.margin - BOX.nodeWidth);
    expect(node(1, '/products').x).toBe((BOX.margin + BOX.width - BOX.margin - BOX.nodeWidth) / 2);
  });

  it('draws a ribbon from the right edge of a page to the left edge of the next, as thick as the sessions', () => {
    const ribbon = layout.links.find((link) => link.source.page === '/' && link.target.page === '/products')!;
    const scale = node(0, '/').height / 100;

    expect(ribbon.thickness).toBeCloseTo(70 * scale);
    expect(ribbon.d.startsWith(`M ${node(0, '/').x + BOX.nodeWidth} `)).toBe(true);
    expect(ribbon.d).toContain(`${node(1, '/products').x} `);
  });

  it('lets the ribbons that leave a page lie one after the other inside it', () => {
    const leaving = layout.links.filter((link) => link.source.page === '/');
    const total = leaving.reduce((sum, link) => sum + link.thickness, 0);

    expect(total).toBeCloseTo(node(0, '/').height);
  });

  it('leaves out a ribbon whose pages are not drawn', () => {
    const bare = layoutSankey(nodes, [...links, { column: 1, source: '/gone', target: '/checkout', sessions: 5 }], 3, BOX);

    expect(bare.links).toHaveLength(3);
  });

  it('does not give a session more height than it may, so that a few of them do not fill the box', () => {
    const few = layoutSankey([{ column: 0, page: '/', sessions: 2, ends: 2 }], [], 1, { ...BOX, maxPerSession: 20 });

    expect(few.nodes[0]?.height).toBe(40);
    expect(few.height).toBe(40);
  });

  it('draws nothing for nothing', () => {
    const empty = layoutSankey([], [], 3, BOX);

    expect(empty.nodes).toEqual([]);
    expect(empty.links).toEqual([]);
    expect(empty.height).toBe(0);
  });
});

describe('the columns of paths that are drawn', () => {
  it('leaves out the columns after the last page anyone got to', () => {
    const used = usedColumns(nodes, links);

    expect(used.columns).toBe(3);
    expect(used.nodes).toEqual(nodes);
    expect(used.links).toEqual(links);
  });

  it('numbers the columns from 0 where the first ones are empty, as going backward leaves them', () => {
    const used = usedColumns(
      [
        { column: 2, page: '/cart', sessions: 4, ends: 4 },
        { column: 3, page: '/checkout', sessions: 4, ends: 0 },
      ],
      [{ column: 2, source: '/cart', target: '/checkout', sessions: 4 }],
    );

    expect(used.columns).toBe(2);
    expect(used.nodes.map((node) => node.column)).toEqual([0, 1]);
    expect(used.links.map((link) => link.column)).toEqual([0]);
  });

  it('is one column for nothing', () => {
    expect(usedColumns([], [])).toEqual({ nodes: [], links: [], columns: 1 });
  });
});

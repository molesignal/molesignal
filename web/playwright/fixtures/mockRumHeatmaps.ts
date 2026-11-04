/**
 * Where people click on a page and how far down they go, by the rules of the server
 * (`infra/rum/query/heatmap.rs`): clicks by the element they landed on and the cell of it, scrolling by
 * the band of depth a view stopped in, a view having seen every band above it.
 */
import type { RumRow } from '../../src/api/rum';

const CELLS = 10;
const BAND_PX = 100;

const numberOf = (value: unknown): number | null => (typeof value === 'number' && Number.isFinite(value) ? value : null);
const textOf = (value: unknown): string | null => (typeof value === 'string' && value !== '' ? value : null);

export function clicksAnswer(rows: readonly RumRow[], limit: number) {
  const elements = new Map<string, RumRow[]>();

  for (const row of rows) {
    if (row['action.type'] !== 'click' && row['action.type'] !== 'tap') continue;

    const key = textOf(row['action.target.path']) ?? textOf(row['action.target.selector']);

    if (key) elements.set(key, [...(elements.get(key) ?? []), row]);
  }

  const mean = (rows: readonly RumRow[], name: string) => {
    const values = rows.map((row) => numberOf(row[name])).filter((value): value is number => value !== null);

    return values.length > 0 ? values.reduce((sum, value) => sum + value, 0) / values.length : null;
  };

  const told = [...elements]
    .map(([element, own]) => {
      const cells = new Map<string, number>();

      for (const row of own) {
        const [x, y, width, height] = ['action.target.offset.x', 'action.target.offset.y', 'action.target.width', 'action.target.height'].map((name) => numberOf(row[name]));

        if (x === null || y === null || !width || !height || x === undefined || y === undefined) continue;

        const cell = [Math.min(CELLS - 1, Math.floor((x * CELLS) / width)), Math.min(CELLS - 1, Math.floor((y * CELLS) / height))].join(',');

        cells.set(cell, (cells.get(cell) ?? 0) + 1);
      }

      return {
        element,
        name: textOf(own[0]?.['action.target.name']),
        selector: textOf(own[0]?.['action.target.selector']),
        clicks: own.length,
        sessions: new Set(own.map((row) => row['session.id'])).size,
        frustrated: own.filter((row) => Number(row['action.frustration.count'] ?? 0) > 0).length,
        pageX: mean(own, 'action.page_position.x'),
        pageY: mean(own, 'action.page_position.y'),
        cells: [...cells].map(([cell, clicks]) => [...cell.split(',').map(Number), clicks]),
      };
    })
    .sort((a, b) => b.clicks - a.clicks || a.element.localeCompare(b.element))
    .slice(0, limit);

  return { clicks: told.reduce((sum, element) => sum + element.clicks, 0), elements: told };
}

export function scrollAnswer(rows: readonly RumRow[]) {
  const depths = rows.map((row) => numberOf(row['view.scroll.max_depth'])).filter((depth): depth is number => depth !== null);

  if (depths.length === 0) return { views: 0, bands: [] };

  const last = Math.max(...depths.map((depth) => Math.floor(depth / BAND_PX)));
  const bands = Array.from({ length: last + 1 }, (_, index) => ({
    depth: index * BAND_PX,
    reached: depths.filter((depth) => Math.floor(depth / BAND_PX) >= index).length / depths.length,
  }));
  const median = bands.filter((band) => band.reached >= 0.5).at(-1);
  const heights = rows.map((row) => numberOf(row['view.scroll.max_scroll_height'])).filter((height): height is number => height !== null);

  return {
    views: depths.length,
    pageHeight: heights.length > 0 ? heights[Math.floor(heights.length / 2)] : null,
    medianDepth: median ? median.depth + BAND_PX : null,
    bands,
  };
}

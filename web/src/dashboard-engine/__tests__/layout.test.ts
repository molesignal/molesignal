import { describe, expect, it } from 'vitest';

import {
  autoLayout,
  clampGridPosition,
  gridPositionsCollide,
  gridSpanSize,
  placeLayoutItem,
} from '../layout';

describe('dashboard grid layout', () => {
  it('includes every internal track gap in a spanned item height', () => {
    expect(gridSpanSize(24, 8, 8)).toBe(376);
    expect(gridSpanSize(1, 8, 8)).toBe(8);
  });

  it('clamps a resized item to configured columns and minimums', () => {
    expect(
      clampGridPosition(
        { x: 23, y: -1, w: 8, h: 1, minH: 4 },
        24,
      ),
    ).toMatchObject({ x: 16, y: 0, w: 8, h: 4 });
  });

  it('pushes colliding items down during a live placement', () => {
    const placed = placeLayoutItem(
      [
        { id: 'a', gridPos: { x: 0, y: 0, w: 12, h: 8 } },
        { id: 'b', gridPos: { x: 12, y: 0, w: 12, h: 8 } },
      ],
      'b',
      { x: 0, y: 0, w: 12, h: 8 },
      24,
    );
    expect(placed.find((item) => item.id === 'a')?.gridPos.y).toBe(8);
    expect(
      gridPositionsCollide(placed[0]!.gridPos, placed[1]!.gridPos),
    ).toBe(false);
  });

  it('moves an item without changing its size', () => {
    const [moved] = placeLayoutItem(
      [{ id: 'panel', gridPos: { x: 0, y: 0, w: 18, h: 24 } }],
      'panel',
      { x: 2, y: 4, w: 18, h: 24 },
      24,
    );

    expect(moved?.gridPos).toEqual({ x: 2, y: 4, w: 18, h: 24 });
  });

  it('resizes an item while preserving its origin', () => {
    const [resized] = placeLayoutItem(
      [{ id: 'panel', gridPos: { x: 2, y: 4, w: 18, h: 24 } }],
      'panel',
      { x: 2, y: 4, w: 20, h: 27 },
      24,
    );

    expect(resized?.gridPos).toEqual({ x: 2, y: 4, w: 20, h: 27 });
  });

  it('packs arbitrary item widths without assuming a two-column layout', () => {
    const result = autoLayout(
      [
        { id: 'a', gridPos: { x: 0, y: 10, w: 8, h: 4 } },
        { id: 'b', gridPos: { x: 0, y: 20, w: 8, h: 4 } },
        { id: 'c', gridPos: { x: 0, y: 30, w: 8, h: 4 } },
      ],
      24,
    );
    expect(result.map((item) => item.gridPos.x)).toEqual([0, 8, 16]);
    expect(result.every((item) => item.gridPos.y === 0)).toBe(true);
  });
});

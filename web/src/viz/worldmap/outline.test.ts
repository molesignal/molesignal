import { describe, expect, it } from 'vitest';

import { contains, distanceTo, outlineOf } from './outline';

// A square of 100 tenths a side, with a square of 20 cut out of its middle.
const SQUARE = 'M100 100l100 0 0 100-100 0z';
const WITH_HOLE = `${SQUARE}M140 140l20 0 0 20-20 0z`;

describe('the outline of a country', () => {
  it('is read from the path the data of the map is written in: each step from the point before it', () => {
    const outline = outlineOf(SQUARE);

    expect(outline.rings).toEqual([
      [
        [100, 100],
        [200, 100],
        [200, 200],
        [100, 200],
      ],
    ]);
    expect([outline.minX, outline.minY, outline.maxX, outline.maxY]).toEqual([100, 100, 200, 200]);
  });

  it('has a ring for each piece, and numbers run together where a minus sign parts them', () => {
    const outline = outlineOf('M0 0l10 5-3 4z');
    const second = outlineOf(`${SQUARE}M300 300l-5 10 20-3z`);

    expect(outline.rings[0]).toEqual([
      [0, 0],
      [10, 5],
      [7, 9],
    ]);
    expect(second.rings).toHaveLength(2);
    expect(second.rings[1]).toEqual([
      [300, 300],
      [295, 310],
      [315, 307],
    ]);
  });

  it('holds a place that is inside a ring, and not one inside a ring cut out of it', () => {
    const outline = outlineOf(WITH_HOLE);

    expect(contains(outline, 120, 120)).toBe(true);
    expect(contains(outline, 150, 150)).toBe(false);
    expect(contains(outline, 250, 150)).toBe(false);
    expect(contains(outline, 50, 50)).toBe(false);
  });

  it('is as far from a place as its nearest edge', () => {
    const outline = outlineOf(SQUARE);

    expect(distanceTo(outline, 150, 90)).toBeCloseTo(10);
    expect(distanceTo(outline, 130, 150)).toBeCloseTo(30);
    expect(distanceTo(outline, 210, 210)).toBeCloseTo(Math.hypot(10, 10));
  });

  it('is empty where the path has no ring', () => {
    const outline = outlineOf('');

    expect(outline.rings).toEqual([]);
    expect(contains(outline, 0, 0)).toBe(false);
    expect(distanceTo(outline, 0, 0)).toBe(Infinity);
  });
});

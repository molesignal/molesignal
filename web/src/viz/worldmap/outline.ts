/** A point on the map, in the units the outline is written in. */
type Point = readonly [x: number, y: number];

/** The rings of a country, and the box that holds them. */
export interface Outline {
  rings: readonly (readonly Point[])[];
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
}

// A ring of the map's data: `M x y l dx dy dx dy … z`, each step from the point before it.
const RING = /M(-?\d+) (-?\d+)l([-\d ]*)z/g;
const NUMBER = /-?\d+/g;

/** The rings of a path written the way the data of the map is, with every point where it lies. */
export function outlineOf(path: string): Outline {
  const rings: Point[][] = [];
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;

  for (const [, startX = '0', startY = '0', steps = ''] of path.matchAll(RING)) {
    let x = Number(startX);
    let y = Number(startY);
    const ring: Point[] = [[x, y]];
    const moves = (steps.match(NUMBER) ?? []).map(Number);

    for (let index = 0; index + 1 < moves.length; index += 2) {
      x += moves[index] ?? 0;
      y += moves[index + 1] ?? 0;
      ring.push([x, y]);
    }

    for (const [pointX, pointY] of ring) {
      minX = Math.min(minX, pointX);
      minY = Math.min(minY, pointY);
      maxX = Math.max(maxX, pointX);
      maxY = Math.max(maxY, pointY);
    }

    rings.push(ring);
  }

  return { rings, minX, minY, maxX, maxY };
}

/** Whether a point is on the land of an outline: inside a ring, and not inside a ring that is a hole in it. */
export function contains(outline: Outline, x: number, y: number): boolean {
  if (x < outline.minX || x > outline.maxX || y < outline.minY || y > outline.maxY) return false;

  let inside = false;

  for (const ring of outline.rings) {
    let [previousX, previousY] = ring.at(-1) ?? [0, 0];

    for (const [currentX, currentY] of ring) {
      if (currentY > y !== previousY > y && x < ((previousX - currentX) * (y - currentY)) / (previousY - currentY) + currentX) inside = !inside;

      previousX = currentX;
      previousY = currentY;
    }
  }

  return inside;
}

/** How far a point is from the nearest edge of an outline. */
export function distanceTo(outline: Outline, x: number, y: number): number {
  let nearest = Infinity;

  for (const ring of outline.rings) {
    let [fromX, fromY] = ring.at(-1) ?? [0, 0];

    for (const [toX, toY] of ring) {
      const dx = toX - fromX;
      const dy = toY - fromY;
      const length = dx * dx + dy * dy;
      // How far along the edge the point's foot is, kept to the edge.
      const along = length === 0 ? 0 : Math.min(1, Math.max(0, ((x - fromX) * dx + (y - fromY) * dy) / length));

      nearest = Math.min(nearest, Math.hypot(x - (fromX + along * dx), y - (fromY + along * dy)));
      fromX = toX;
      fromY = toY;
    }
  }

  return nearest;
}

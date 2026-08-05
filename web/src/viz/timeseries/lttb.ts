/**
 * Largest-Triangle-Three-Buckets downsampling (Sveinn Steinarsson, 2013).
 *
 * Reduces an ordered series of (t, v) points to `targetSize` while preserving
 * the visual envelope of the data. First and last points are always retained.
 *
 * Input must be sorted by `t` ascending. NaN or non-finite `v` are skipped.
 *
 * Complexity: O(n).
 */
export function lttb(points: ReadonlyArray<[number, number]>, targetSize: number): Array<[number, number]> {
  if (targetSize >= points.length || targetSize < 3) {
    return points.slice();
  }

  const sampled: Array<[number, number]> = [];
  const bucketSize = (points.length - 2) / (targetSize - 2);

  // Always include the first point.
  sampled.push(points[0]!);
  let a = 0;

  for (let i = 0; i < targetSize - 2; i++) {
    // Compute average for the next bucket.
    const avgRangeStart = Math.floor((i + 1) * bucketSize) + 1;
    const avgRangeEnd = Math.min(Math.floor((i + 2) * bucketSize) + 1, points.length);
    let avgX = 0;
    let avgY = 0;
    let avgCount = 0;
    for (let j = avgRangeStart; j < avgRangeEnd; j++) {
      const p = points[j]!;
      if (!Number.isFinite(p[1])) continue;
      avgX += p[0];
      avgY += p[1];
      avgCount += 1;
    }
    if (avgCount > 0) {
      avgX /= avgCount;
      avgY /= avgCount;
    }

    // Iterate the current bucket and find the point that forms the largest
    // triangle with point `a` and the average of the next bucket.
    const rangeStart = Math.floor(i * bucketSize) + 1;
    const rangeEnd = Math.floor((i + 1) * bucketSize) + 1;
    const pa = points[a]!;
    let maxArea = -1;
    let nextA = rangeStart;
    for (let j = rangeStart; j < rangeEnd; j++) {
      const p = points[j]!;
      if (!Number.isFinite(p[1])) continue;
      const area = Math.abs((pa[0] - avgX) * (p[1] - pa[1]) - (pa[0] - p[0]) * (avgY - pa[1])) * 0.5;
      if (area > maxArea) {
        maxArea = area;
        nextA = j;
      }
    }
    sampled.push(points[nextA]!);
    a = nextA;
  }

  // Always include the last point.
  sampled.push(points[points.length - 1]!);
  return sampled;
}

/**
 * Convenience: convert uPlot column-major data `[xs, ys1, ys2, ...]` into a
 * downsampled version. Each series is downsampled independently against the
 * shared x-axis (NaN slots in a series do not prevent that series's
 * downsample, but they shrink its contribution).
 */
export function downsampleSeries(
  data: ReadonlyArray<ReadonlyArray<number>>,
  targetSize: number,
): number[][] {
  const xs = data[0]!;
  if (xs.length <= targetSize) return data.map((row) => Array.from(row));
  const out: number[][] = [Array.from(xs).slice(0, 0)];
  for (let s = 1; s < data.length; s++) {
    const ys = data[s]!;
    const paired: Array<[number, number]> = new Array(xs.length);
    for (let i = 0; i < xs.length; i++) paired[i] = [xs[i]!, ys[i]!];
    const reduced = lttb(paired, targetSize);
    if (s === 1) {
      out[0] = reduced.map((p) => p[0]);
    }
    out.push(reduced.map((p) => p[1]));
  }
  return out;
}

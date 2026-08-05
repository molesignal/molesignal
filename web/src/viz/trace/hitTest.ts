import type { LaidOutTrace, SpanNode, Viewport } from './types';

/**
 * O(log n) hit-test using per-row sorted indexes built once. Spans within a
 * row are sorted by start_ns; we binary-search for the entry that contains
 * the query x.
 */
export class HitTester {
  private rows: Map<number, SpanNode[]> = new Map();

  constructor(layout: LaidOutTrace) {
    for (const n of layout.nodes) {
      let bucket = this.rows.get(n.rowIndex);
      if (!bucket) {
        bucket = [];
        this.rows.set(n.rowIndex, bucket);
      }
      bucket.push(n);
    }
    for (const bucket of this.rows.values()) {
      bucket.sort((a, b) => a.startOffsetNs - b.startOffsetNs);
    }
  }

  /**
   * @param x  CSS pixels from left edge of canvas
   * @param y  CSS pixels from top edge
   */
  hit(
    x: number,
    y: number,
    viewport: Viewport,
    rowHeight: number,
    width: number,
  ): SpanNode | null {
    const row = Math.floor(y / rowHeight + viewport.scrollRow);
    const bucket = this.rows.get(row);
    if (!bucket) return null;
    const range = viewport.toNs - viewport.fromNs;
    if (range <= 0) return null;
    const xScale = width / range;
    const ns = viewport.fromNs + x / xScale;

    // Binary search for the rightmost span whose startOffsetNs <= ns.
    let lo = 0;
    let hi = bucket.length - 1;
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1;
      if (bucket[mid]!.startOffsetNs <= ns) lo = mid;
      else hi = mid - 1;
    }
    const cand = bucket[lo];
    if (!cand) return null;
    if (cand.startOffsetNs <= ns && cand.startOffsetNs + cand.durationNs >= ns) return cand;
    return null;
  }
}

import type { DiffFlamebearer, Flamebearer } from '@/api/profiles';
import type { PaletteKey } from '@/viz/timeseries/themeAdapter';

/**
 * Pure flamebearer helpers — decoding, layout math, coloring and value
 * formatting. Kept free of React so the tricky bits (relative-offset decoding,
 * zoom containment) are unit-testable in isolation from rendering.
 *
 * Flamebearer `levels[d]` is a flat int array of bars. A single-window bar is
 * 4 ints `[offset, total, self, nameIndex]`; a diff bar is 5 ints with a
 * trailing signed `delta`. `offset` is the gap from the previous bar's right
 * edge at the same depth (Pyroscope-compatible encoding).
 */

export const SINGLE_STRIDE = 4;
export const DIFF_STRIDE = 5;

export interface FlameNode {
  depth: number;
  /** Absolute start in value units (left edge). */
  start: number;
  total: number;
  self: number;
  nameIndex: number;
  /** comparison − baseline; present only for diff flamebearers. */
  delta?: number;
}

/** Decode the relative-offset `levels[]` into absolute-positioned nodes. */
export function decodeNodes(levels: number[][], diff: boolean): FlameNode[] {
  const stride = diff ? DIFF_STRIDE : SINGLE_STRIDE;
  const nodes: FlameNode[] = [];
  for (let depth = 0; depth < levels.length; depth++) {
    const row = levels[depth] ?? [];
    let prevEnd = 0;
    for (let i = 0; i + stride <= row.length; i += stride) {
      const start = prevEnd + (row[i] ?? 0);
      const total = row[i + 1] ?? 0;
      const node: FlameNode = {
        depth,
        start,
        total,
        self: row[i + 2] ?? 0,
        nameIndex: row[i + 3] ?? 0,
      };
      if (diff) node.delta = row[i + 4] ?? 0;
      nodes.push(node);
      prevEnd = start + total;
    }
  }
  return nodes;
}

export interface FlameWindow {
  depth: number;
  start: number;
  total: number;
}

export const ROOT_WINDOW: FlameWindow = { depth: 0, start: 0, total: 0 };

/** A node is inside the focus window when it sits at/below the focus depth and
 *  its span is contained by the focus span. */
export function nodeInWindow(node: FlameNode, win: FlameWindow): boolean {
  return (
    node.depth >= win.depth &&
    node.start < win.start + win.total &&
    node.start + node.total > win.start
  );
}

/** The ancestor of the focus window at a shallower depth (the bar that contains
 *  it). Returns null when none — e.g. a malformed tree. */
export function ancestorAtDepth(
  nodes: FlameNode[],
  win: FlameWindow,
  depth: number,
): FlameNode | null {
  for (const n of nodes) {
    if (n.depth !== depth) continue;
    if (n.start <= win.start && win.start + win.total <= n.start + n.total) return n;
  }
  return null;
}

const FRAME_COLOR_KEYS: PaletteKey[] = [
  '--accent',
  '--blue',
  '--green',
  '--yellow',
  '--red',
  '--purple',
  '--primary',
];

const frameColorCache = new Map<string, PaletteKey>();

/** Stable hash-based frame → palette key, so a function keeps its color. */
export function colorKeyForFrame(name: string): PaletteKey {
  const cached = frameColorCache.get(name);
  if (cached) return cached;
  let h = 0;
  for (let i = 0; i < name.length; i++) {
    h = (h * 31 + name.charCodeAt(i)) | 0;
  }
  const key = FRAME_COLOR_KEYS[Math.abs(h) % FRAME_COLOR_KEYS.length]!;
  frameColorCache.set(name, key);
  return key;
}

export type DiffTone = 'increase' | 'decrease' | 'neutral';

export function diffTone(delta: number | undefined): DiffTone {
  if (!delta || delta === 0) return 'neutral';
  return delta > 0 ? 'increase' : 'decrease';
}

/** 0..1 intensity for a diff bar relative to the largest absolute delta. */
export function diffIntensity(delta: number | undefined, maxAbsDelta: number): number {
  if (!delta || maxAbsDelta <= 0) return 0;
  return Math.min(1, Math.abs(delta) / maxAbsDelta);
}

/** Human-readable value, picked by the flamebearer `units`. */
export function formatValue(value: number, units: string): string {
  const u = units.toLowerCase();
  if (u === 'nanoseconds' || u === 'ns') return formatDuration(value);
  if (u === 'bytes') return formatBytes(value);
  return `${formatCount(value)}${u && u !== 'count' && u !== 'samples' ? ` ${units}` : ''}`;
}

export function formatDuration(ns: number): string {
  const abs = Math.abs(ns);
  if (abs < 1_000) return `${Math.round(ns)} ns`;
  if (abs < 1_000_000) return `${(ns / 1_000).toFixed(1)} µs`;
  if (abs < 1_000_000_000) return `${(ns / 1_000_000).toFixed(1)} ms`;
  return `${(ns / 1_000_000_000).toFixed(2)} s`;
}

export function formatBytes(bytes: number): string {
  const abs = Math.abs(bytes);
  if (abs < 1024) return `${Math.round(bytes)} B`;
  if (abs < 1024 ** 2) return `${(bytes / 1024).toFixed(1)} KiB`;
  if (abs < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(1)} MiB`;
  return `${(bytes / 1024 ** 3).toFixed(2)} GiB`;
}

export function formatCount(n: number): string {
  const abs = Math.abs(n);
  if (abs < 1_000) return `${Math.round(n)}`;
  if (abs < 1_000_000) return `${(n / 1_000).toFixed(1)}K`;
  if (abs < 1_000_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  return `${(n / 1_000_000_000).toFixed(2)}B`;
}

export function rootTotal(fb: Flamebearer | DiffFlamebearer): number {
  return fb.numTicks || (fb.levels[0]?.[1] ?? 0);
}

// ── self-time heatmap ────────────────────────────────────────────────────────
// Width already encodes total time; color encodes SELF time, so "wide + hot" =
// a real hotspot and "wide + cold" = time spent in children. A cool→hot ramp
// (blue → amber → red) keeps the alarming red meaningful (= look here) and off
// the random per-frame palette that clashed with the product's "red = alert".
const HEAT_STOPS: Array<[number, number, number]> = [
  [56, 130, 184], // cold — self ≈ 0 (time is in children)
  [224, 178, 70], // warm
  [214, 78, 56], // hot — the self-time hotspot
];

/** Map a 0..1 self ratio to a cool→hot `rgb()` string. */
export function heatColor(ratio: number): string {
  const r = Math.max(0, Math.min(1, Number.isFinite(ratio) ? ratio : 0));
  const seg = r >= 0.5 ? 1 : 0;
  const t = seg === 1 ? (r - 0.5) * 2 : r * 2;
  const a = HEAT_STOPS[seg]!;
  const b = HEAT_STOPS[seg + 1]!;
  const mix = (i: number) => Math.round(a[i]! + (b[i]! - a[i]!) * t);
  return `rgb(${mix(0)}, ${mix(1)}, ${mix(2)})`;
}

// ── top functions (flat / self time) ─────────────────────────────────────────
export interface TopFunction {
  name: string;
  /** Σ self across every frame of this function (pprof "flat"). */
  self: number;
  /** Σ total across every frame (pprof "cum"); may over-count recursion. */
  total: number;
  /** self as a percentage of the root total. */
  selfPct: number;
  /** cumulative value as a percentage of the root total. */
  totalPct: number;
}

/** Aggregate self/total per function name, sorted by self time desc. The root
 *  ("total") frame is dropped — it isn't a real function. */
export function topFunctions(fb: Flamebearer | DiffFlamebearer, diff = false): TopFunction[] {
  const nodes = decodeNodes(fb.levels, diff);
  const root = rootTotal(fb) || 1;
  const acc = new Map<string, { self: number; total: number }>();
  for (const n of nodes) {
    const name = fb.names[n.nameIndex] ?? '';
    if (!name || (n.depth === 0 && name === 'total')) continue;
    const e = acc.get(name) ?? { self: 0, total: 0 };
    e.self += n.self;
    e.total += n.total;
    acc.set(name, e);
  }
  return [...acc.entries()]
    .map(([name, e]) => ({
      name,
      self: e.self,
      total: e.total,
      selfPct: (e.self / root) * 100,
      totalPct: (e.total / root) * 100,
    }))
    .sort((a, b) => b.self - a.self);
}

// ── call tree (aggregated, for the tree view) ────────────────────────────────
export const nodeKey = (n: FlameNode): string => `${n.depth}:${n.start}`;

/** Parent-key → children, reconstructed from the flat nodes by span containment.
 *  Depth-0 nodes live under the synthetic `'root'` key. Siblings are widest-first
 *  so the tree order matches the flamegraph. */
export function callTreeChildren(nodes: FlameNode[]): Map<string, FlameNode[]> {
  let maxDepth = 0;
  for (const n of nodes) maxDepth = Math.max(maxDepth, n.depth);
  const byDepth: FlameNode[][] = Array.from({ length: maxDepth + 1 }, () => []);
  for (const n of nodes) byDepth[n.depth]!.push(n);

  const map = new Map<string, FlameNode[]>();
  map.set('root', [...(byDepth[0] ?? [])]);
  for (let d = 1; d <= maxDepth; d++) {
    for (const child of byDepth[d] ?? []) {
      const parent = (byDepth[d - 1] ?? []).find(
        (p) => p.start <= child.start && child.start + child.total <= p.start + p.total,
      );
      if (!parent) continue;
      const pk = nodeKey(parent);
      const list = map.get(pk) ?? [];
      list.push(child);
      map.set(pk, list);
    }
  }
  for (const list of map.values()) list.sort((a, b) => b.total - a.total);
  return map;
}

// ── profile_type → value unit (the list rows don't carry units) ──────────────
export function unitForProfileType(type: string): string {
  switch (type) {
    case 'cpu':
    case 'wall':
    case 'lock':
      return 'nanoseconds';
    case 'alloc_space':
    case 'inuse_space':
      return 'bytes';
    case 'alloc_objects':
    case 'inuse_objects':
    case 'goroutines':
      return 'count';
    default:
      return 'samples';
  }
}

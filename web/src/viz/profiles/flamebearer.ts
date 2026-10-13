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

/** The name the root of every flame graph comes with: all the samples are under it. */
export const ROOT_FRAME = 'total';

/**
 * What the root is shown as. Its own name is also the name of a measure, so a
 * root called "total" read "total: total 31.85 s" and led a call path as if it
 * were a function. "/" is where a path starts, in any language.
 */
export const ROOT_LABEL = '/';

export function isRootFrame(name: string, depth: number): boolean {
  return depth === 0 && name === ROOT_FRAME;
}

/** The name a frame is shown under: its function, or the root's sign. */
export function frameLabel(name: string, depth: number): string {
  return isRootFrame(name, depth) ? ROOT_LABEL : name;
}

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

/** A figure with as many decimals as it has, up to `digits`: 60 and 1.5, not 60.00 and 1.50. */
function trimmed(value: number, digits: number): string {
  return String(Number(value.toFixed(digits)));
}

/**
 * A duration in the unit it is read in: a minute of capture is `1 min`, not
 * `60.00 s`, and 48 of them are `48 min`, not `2880.00 s`.
 */
export function formatDuration(ns: number): string {
  const abs = Math.abs(ns);
  if (abs < 1_000) return `${Math.round(ns)} ns`;
  if (abs < 1_000_000) return `${trimmed(ns / 1_000, 1)} µs`;
  if (abs < 1_000_000_000) return `${trimmed(ns / 1_000_000, 1)} ms`;
  if (abs < 60_000_000_000) return `${trimmed(ns / 1_000_000_000, 2)} s`;
  if (abs < 3_600_000_000_000) return `${trimmed(ns / 60_000_000_000, 1)} min`;
  return `${trimmed(ns / 3_600_000_000_000, 1)} h`;
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
    if (!name || isRootFrame(name, n.depth)) continue;
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

// ── what changed between two periods, function by function ─────────────────
export interface DiffFunction {
  name: string;
  /** The function's own cost in the baseline period, and in the one compared with it. */
  baseline: number;
  comparison: number;
  /** comparison − baseline. */
  delta: number;
  /** The change as a share of the baseline; `null` where the function did not run in it. */
  deltaPct: number | null;
}

/**
 * The functions of a diff by how much their own cost changed, largest change
 * first, whichever way it went. Functions that did not change are left out.
 *
 * A bar of a diff carries both periods summed and how the second differs from
 * the first, both for the frame with everything it calls. A function's own
 * part of each is what is left once its callees' are taken off.
 */
export function diffFunctions(fb: DiffFlamebearer): DiffFunction[] {
  const nodes = decodeNodes(fb.levels, true);
  const children = callTreeChildren(nodes);
  const byName = new Map<string, { summed: number; delta: number }>();

  for (const node of nodes) {
    const name = fb.names[node.nameIndex] ?? '';
    if (!name || isRootFrame(name, node.depth)) continue;

    const below = (children.get(nodeKey(node)) ?? []).reduce((sum, child) => sum + (child.delta ?? 0), 0);
    const entry = byName.get(name) ?? { summed: 0, delta: 0 };
    entry.summed += node.self;
    entry.delta += (node.delta ?? 0) - below;
    byName.set(name, entry);
  }

  return [...byName.entries()]
    .filter(([, entry]) => entry.delta !== 0)
    .map(([name, entry]) => {
      // summed = baseline + comparison, delta = comparison − baseline.
      const baseline = Math.max(0, (entry.summed - entry.delta) / 2);
      const comparison = Math.max(0, (entry.summed + entry.delta) / 2);

      return {
        name,
        baseline,
        comparison,
        delta: entry.delta,
        deltaPct: baseline > 0 ? (entry.delta / baseline) * 100 : null,
      };
    })
    .sort((a, b) => Math.abs(b.delta) - Math.abs(a.delta));
}

export interface DiffTotals {
  baseline: number;
  comparison: number;
  /** comparison − baseline. */
  delta: number;
  /** The change as a share of the baseline; `null` where the baseline is empty. */
  deltaPct: number | null;
}

/**
 * What each period of a diff adds up to. The root carries both summed and how
 * the second differs from the first, which is enough to tell them apart.
 */
export function diffTotals(fb: DiffFlamebearer): DiffTotals {
  const summed = rootTotal(fb);
  const delta = fb.levels[0]?.[DIFF_STRIDE - 1] ?? 0;
  const baseline = Math.max(0, (summed - delta) / 2);

  return {
    baseline,
    comparison: Math.max(0, (summed + delta) / 2),
    delta,
    deltaPct: baseline > 0 ? (delta / baseline) * 100 : null,
  };
}

/** A change with its direction: `+1.2 s`, `-340 ms`. */
export function formatChange(delta: number, units: string): string {
  return `${delta > 0 ? '+' : ''}${formatValue(delta, units)}`;
}

/** A change as a share of what it changed from: `+11.2%`, `-3.4%`, `+240%`. */
export function formatChangePct(pct: number): string {
  const figure = Math.abs(pct) >= 100 ? Math.round(pct).toString() : trimmed(pct, 1);

  return `${pct > 0 ? '+' : ''}${figure}%`;
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

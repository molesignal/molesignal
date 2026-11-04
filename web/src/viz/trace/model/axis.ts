/** The steps a time axis is marked in: 1, 2 and 5 of every power of ten nanoseconds. */
const STEPS = [1, 2, 5] as const;

/** The length between two marks of an axis `spanNs` long and `widthPx` wide, marks at least `gapPx` apart. */
export function tickStep(spanNs: number, widthPx: number, gapPx = 88): number {
  if (!(spanNs > 0) || !(widthPx > 0)) return Math.max(1, spanNs);

  const least = (spanNs * gapPx) / widthPx;
  let power = 10 ** Math.floor(Math.log10(Math.max(1, least)));

  for (;;) {
    for (const step of STEPS) {
      if (step * power >= least) return step * power;
    }
    power *= 10;
  }
}

/**
 * Where the marks of a time axis stand, as offsets from the start of the
 * trace: on round lengths of time inside `[fromNs, toNs]`, so a reader can
 * count along them. The start of the window always has one.
 */
export function timeTicks(fromNs: number, toNs: number, widthPx: number, gapPx?: number): number[] {
  const spanNs = Math.max(1, toNs - fromNs);
  const step = tickStep(spanNs, widthPx, gapPx);
  const ticks = [fromNs];

  for (let tick = Math.ceil(fromNs / step) * step; tick <= toNs; tick += step) {
    // A mark less than half a step past the start would sit on top of the start's.
    if (tick - fromNs >= step / 2) ticks.push(tick);
  }

  return ticks;
}

/** Where an offset stands across a window, in percent; outside the window it is clamped to its edge. */
export function percentAcross(offsetNs: number, fromNs: number, toNs: number): number {
  const spanNs = toNs - fromNs;
  if (!(spanNs > 0)) return 0;
  return Math.min(100, Math.max(0, ((offsetNs - fromNs) / spanNs) * 100));
}

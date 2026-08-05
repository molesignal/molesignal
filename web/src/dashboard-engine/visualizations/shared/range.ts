export interface ValueRange {
  min: number;
  max: number;
}

export function normalizeValueRange(
  configuredMin: number | undefined,
  configuredMax: number | undefined,
  value: number,
): ValueRange {
  const safeValue = Number.isFinite(value) ? value : 0;
  let min = finiteOr(configuredMin, 0);
  let max = finiteOr(configuredMax, Math.max(100, safeValue));

  if (min > max) [min, max] = [max, min];
  if (min === max) {
    const padding = Math.max(Math.abs(min) * 0.1, 1);
    min -= padding;
    max += padding;
  }
  return { min, max };
}

export function zeroInclusiveRange(values: readonly number[]): ValueRange {
  const finite = values.filter(Number.isFinite);
  if (finite.length === 0) return { min: 0, max: 1 };
  let min = Math.min(0, ...finite);
  let max = Math.max(0, ...finite);
  if (min === max) {
    const padding = Math.max(Math.abs(min) * 0.1, 1);
    min -= padding;
    max += padding;
  }
  return { min, max };
}

export function valueRatio(value: number, range: ValueRange): number {
  if (!Number.isFinite(value)) return 0;
  return clamp((value - range.min) / (range.max - range.min), 0, 1);
}

export function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

function finiteOr(value: number | undefined, fallback: number): number {
  return typeof value === 'number' && Number.isFinite(value)
    ? value
    : fallback;
}

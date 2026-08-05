export function normalizeTimestamp(value: unknown): number | null {
  if (typeof value === 'number' && Number.isFinite(value)) {
    const magnitude = Math.abs(value);
    if (magnitude >= 1e14) return value / 1_000_000;
    if (magnitude >= 1e11) return value / 1_000;
    return value;
  }
  if (typeof value !== 'string' || value.trim() === '') return null;
  const numeric = Number(value);
  if (Number.isFinite(numeric)) return normalizeTimestamp(numeric);
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) ? parsed / 1_000 : null;
}

export function medianPositiveStep(values: readonly number[]): number {
  const steps = values
    .slice(1)
    .map((value, index) => value - values[index]!)
    .filter((value) => Number.isFinite(value) && value > 0)
    .sort((left, right) => left - right);
  if (steps.length === 0) return 1;
  const middle = Math.floor(steps.length / 2);
  return steps.length % 2 === 0
    ? (steps[middle - 1]! + steps[middle]!) / 2
    : steps[middle]!;
}

export function normalizedTimelinePositions(
  values: readonly unknown[] | undefined,
  length: number,
): { values: number[]; usesTime: boolean } {
  const parsed = (values ?? []).slice(0, length).map(normalizeTimestamp);
  if (parsed.length !== length || parsed.some((value) => value === null)) {
    return {
      values: Array.from({ length }, (_, index) => index),
      usesTime: false,
    };
  }
  const normalized = parsed as number[];
  const monotonic = normalized.every(
    (value, index) => index === 0 || value >= normalized[index - 1]!,
  );
  return monotonic
    ? { values: normalized, usesTime: true }
    : {
        values: Array.from({ length }, (_, index) => index),
        usesTime: false,
      };
}

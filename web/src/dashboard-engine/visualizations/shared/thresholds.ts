import type { ValueRange } from './range';
import type { ThresholdConfig } from '../../schema';

export interface ThresholdInterval {
  start: number;
  end: number;
  color: string;
  label?: string | undefined;
}

interface NormalizedThresholdStep {
  value: number;
  color: string;
  label?: string | undefined;
  order: number;
}

export function buildThresholdIntervals(
  thresholds: ThresholdConfig | undefined,
  range: ValueRange,
): ThresholdInterval[] {
  const steps = normalizeThresholdSteps(thresholds, range);
  const first = steps[0];
  if (!first) return [];

  let active = first;
  for (const step of steps) {
    if (step.value > range.min) break;
    active = step;
  }

  const intervals: ThresholdInterval[] = [];
  let cursor = range.min;
  for (const step of steps) {
    if (step.value <= range.min) continue;
    if (step.value >= range.max) break;
    if (step.value > cursor) {
      intervals.push(toInterval(cursor, step.value, active));
      cursor = step.value;
    }
    active = step;
  }
  if (cursor < range.max) intervals.push(toInterval(cursor, range.max, active));
  return intervals;
}

export function resolveThresholdColor(
  value: number,
  thresholds: ThresholdConfig | undefined,
  range: ValueRange,
): string | undefined {
  const steps = normalizeThresholdSteps(thresholds, range);
  let active = steps[0];
  for (const step of steps) {
    if (step.value > value) break;
    active = step;
  }
  return active?.color;
}

export function thresholdMarkerValues(
  thresholds: ThresholdConfig | undefined,
  range: ValueRange,
): number[] {
  return normalizeThresholdSteps(thresholds, range)
    .map((step) => step.value)
    .filter((value) => value > range.min && value < range.max);
}

function normalizeThresholdSteps(
  thresholds: ThresholdConfig | undefined,
  range: ValueRange,
): NormalizedThresholdStep[] {
  if (!thresholds) return [];
  return thresholds.steps
    .map((step, order): NormalizedThresholdStep | null => {
      if (step.value === null) {
        return { ...step, value: Number.NEGATIVE_INFINITY, order };
      }
      if (!Number.isFinite(step.value)) return null;
      const value =
        thresholds.mode === 'percentage'
          ? range.min + (step.value / 100) * (range.max - range.min)
          : step.value;
      return { ...step, value, order };
    })
    .filter((step): step is NormalizedThresholdStep => step !== null)
    .sort((left, right) => left.value - right.value || left.order - right.order);
}

function toInterval(
  start: number,
  end: number,
  step: NormalizedThresholdStep,
): ThresholdInterval {
  return {
    start,
    end,
    color: step.color,
    ...(step.label ? { label: step.label } : {}),
  };
}

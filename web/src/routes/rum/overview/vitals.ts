import type { ExperienceGrade } from '@/api/rum';

export type VitalKey = 'lcp' | 'inp' | 'cls';

/**
 * Where a Core Web Vital stops being good and where it becomes poor, at the
 * 75th percentile (the thresholds the web-vitals programme publishes). LCP and
 * INP are in milliseconds; CLS has no unit.
 */
export const VITAL_THRESHOLDS: Record<VitalKey, { good: number; poor: number }> = {
  lcp: { good: 2_500, poor: 4_000 },
  inp: { good: 200, poor: 500 },
  cls: { good: 0.1, poor: 0.25 },
};

export function vitalGrade(key: VitalKey, value: number): ExperienceGrade {
  if (!Number.isFinite(value) || value <= 0) return 'unknown';
  const { good, poor } = VITAL_THRESHOLDS[key];
  if (value > poor) return 'poor';
  if (value > good) return 'needs_improvement';
  return 'good';
}

export interface VitalScale {
  /** The share of the scale each band takes, in percent; they add up to 100. */
  bands: { good: number; needsImprovement: number; poor: number };
  /** Where the value sits along the scale, in percent; `null` when there is no value. */
  position: number | null;
}

// The scale runs half as far again as the "poor" line, so a poor value still
// has somewhere to be on it; anything beyond pins to the end.
const SCALE_REACH = 1.5;

export function vitalScale(key: VitalKey, value: number): VitalScale {
  const { good, poor } = VITAL_THRESHOLDS[key];
  const end = poor * SCALE_REACH;
  const bands = {
    good: (good / end) * 100,
    needsImprovement: ((poor - good) / end) * 100,
    poor: ((end - poor) / end) * 100,
  };
  if (!Number.isFinite(value) || value <= 0) return { bands, position: null };
  return { bands, position: Math.min(100, (value / end) * 100) };
}

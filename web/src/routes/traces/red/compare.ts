import type { TraceRedChoice, TraceRedFigures } from '@/api/traceRed';

/** Which requests the comparison sets apart from the rest. */
export type CompareChoice = 'failed' | 'p95' | 'p99';

export const COMPARE_CHOICES: readonly CompareChoice[] = ['failed', 'p95', 'p99'];

/**
 * What to ask for a choice, given the figures of every request: the failed
 * ones, or those at least as slow as the 95th or 99th percentile. `null`
 * where the figures cannot tell that set: no failure, or no duration.
 */
export function choiceParams(choice: CompareChoice | null, total: TraceRedFigures): TraceRedChoice | null {
  if (choice === 'failed') return total.errors ? { errors: true } : null;

  const threshold = choice === 'p95' ? total.p95Ns : choice === 'p99' ? total.p99Ns : null;

  return threshold !== null && threshold > 0 ? { minDuration: Math.round(threshold) } : null;
}

/** A share from 0 to 1 as a whole percentage, or with a decimal below ten. */
export function formatShare(share: number): string {
  const percent = share * 100;

  if (percent === 0) return '0%';
  if (percent < 10) return `${percent.toFixed(1)}%`;

  return `${Math.round(percent)}%`;
}

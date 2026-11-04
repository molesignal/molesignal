import type { SeverityThreshold } from '@/types/alerting';

import { conditionMatches } from '../alertRuleModel';

export interface BacktestSpan {
  from: number;
  to: number;
}

export interface RuleBacktest {
  /** From crossing the line until the rule holds, or until it fell back without holding. */
  pending: BacktestSpan[];
  /** From the moment the rule would have fired until the value came back. */
  firing: BacktestSpan[];
  /** How many times it crossed the line and came back too soon to fire; one still over at the end has not. */
  nearMisses: number;
}

/**
 * What a threshold would have done over a stretch of samples: the same count the
 * evaluator keeps, a run of consecutive rounds over the line, so moving the line
 * or the rounds it must hold redraws the answer without asking the server again.
 */
export function backtestRule(
  values: readonly number[],
  timestamps: readonly number[],
  band: SeverityThreshold,
): RuleBacktest {
  const required = Math.max(1, band.forPeriods);
  const result: RuleBacktest = { pending: [], firing: [], nearMisses: 0 };
  const timeOf = (index: number) => timestamps[Math.min(index, timestamps.length - 1)] ?? 0;

  let runStart = -1;
  let firedAt = -1;

  const closeRun = (end: number, cameBack: boolean) => {
    if (runStart < 0) return;

    if (firedAt < 0) {
      result.pending.push({ from: timeOf(runStart), to: timeOf(end) });
      if (cameBack) result.nearMisses += 1;
    } else {
      if (firedAt > runStart) result.pending.push({ from: timeOf(runStart), to: timeOf(firedAt) });
      result.firing.push({ from: timeOf(firedAt), to: timeOf(end) });
    }

    runStart = -1;
    firedAt = -1;
  };

  values.forEach((value, index) => {
    if (!conditionMatches(value, band.operator, band.threshold)) {
      closeRun(index, true);
      return;
    }

    if (runStart < 0) runStart = index;
    if (firedAt < 0 && index - runStart + 1 >= required) firedAt = index;
  });
  closeRun(values.length - 1, false);

  return result;
}

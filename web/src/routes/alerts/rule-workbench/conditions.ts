import type { AlertRule, AlertRuleKind, NoDataAction, SeverityThreshold } from '@/types/alerting';

import { judgesItself, withoutQuery } from './kinds';

export const NO_DATA_ACTIONS: NoDataAction[] = ['resolve', 'keep', 'alert', 'zero'];

/** A rule's recovery condition and what a round without a value counts as, as the editor holds them. */
export interface RuleConditions {
  /** The recovery threshold as typed; empty when an incident recovers at the threshold itself. */
  recoveryThreshold: string;
  /** Rounds on the healthy side in a row before an incident recovers. */
  recoveryPeriods: number;
  noDataAction: NoDataAction;
  /** Rounds without a value in a row before a no-data incident opens. */
  noDataPeriods: number;
}

/** What a rule of `kind` does without a value when nobody chose: a threshold rule recovers, an anomaly rule keeps its state. */
export function defaultNoDataAction(kind: AlertRuleKind | undefined): NoDataAction {
  return kind === 'anomaly' ? 'keep' : 'resolve';
}

/** Rounds of `periodSecs` that cover `minutes`, at least one. */
export function periodsFor(minutes: number, periodSecs: number): number {
  return Math.max(1, Math.ceil((minutes * 60) / periodSecs));
}

/** The minutes `periods` rounds of `periodSecs` take, at least one. */
export function minutesFor(periods: number, periodSecs: number): number {
  return Math.max(1, Math.round((periods * periodSecs) / 60));
}

/** The conditions of `rule`, or a new rule's: recovery at the threshold, the kind's default without data, five minutes before a no-data incident. */
export function conditionsOf(rule: AlertRule | undefined, periodSecs: number): RuleConditions {
  const recovery = rule?.trigger.recovery;
  const noData = rule?.trigger.noData;

  return {
    recoveryThreshold: recovery?.threshold == null ? '' : String(recovery.threshold),
    recoveryPeriods: Math.max(1, recovery?.forPeriods ?? 1),
    noDataAction: noData?.action ?? defaultNoDataAction(rule?.kind),
    noDataPeriods: noData?.forPeriods ? noData.forPeriods : periodsFor(5, periodSecs),
  };
}

/** The `recovery` and `no_data` a rule is saved with; a condition left as it would be anyway is left out. */
export function conditionsPayload(conditions: RuleConditions, kind: AlertRuleKind | undefined) {
  // A rule that judges itself has no threshold to recover past; one without a query always has a value.
  const line = kind && judgesItself(kind) ? '' : conditions.recoveryThreshold.trim();
  const keepsNoData = !kind || !withoutQuery(kind);

  return {
    recovery:
      line || conditions.recoveryPeriods > 1
        ? { threshold: line ? Number(line) : null, forPeriods: conditions.recoveryPeriods }
        : null,
    noData:
      !keepsNoData || conditions.noDataAction === defaultNoDataAction(kind)
        ? null
        : {
            action: conditions.noDataAction,
            forPeriods: conditions.noDataAction === 'alert' ? conditions.noDataPeriods : 0,
          },
  };
}

/**
 * A recovery threshold that cannot stand where it is: it has to be on the healthy side of every
 * band (no higher than one the value fires above, no lower than one it fires below), and the bands
 * have to fire on one side. The server checks the same.
 */
export function recoveryLineInvalid(bands: SeverityThreshold[], recoveryThreshold: string): boolean {
  const text = recoveryThreshold.trim();
  if (!text) return false;

  const line = Number(text);
  if (!Number.isFinite(line)) return true;

  if (bands.every((band) => band.operator === 'gt' || band.operator === 'gte')) {
    return !bands.every((band) => line <= band.threshold);
  }

  if (bands.every((band) => band.operator === 'lt' || band.operator === 'lte')) {
    return !bands.every((band) => line >= band.threshold);
  }

  return true;
}

import type {
  AlertRule,
  AlertRuleKind,
  CompositeQuery,
  ForecastParams,
  HeartbeatParams,
  OutlierParams,
} from '@/types/alerting';

/** The ways a rule made in the workbench fires; anomaly rules have a page of their own. */
export const WORKBENCH_KINDS = ['scheduled', 'forecast', 'outlier', 'composite', 'heartbeat'] as const;
export type WorkbenchKind = (typeof WORKBENCH_KINDS)[number];

export function isWorkbenchKind(kind: AlertRuleKind | undefined): kind is WorkbenchKind {
  return (WORKBENCH_KINDS as readonly string[]).includes(kind ?? 'scheduled');
}

/** Kinds that judge a round themselves: one severity, no graded thresholds, no recovery line. */
export function judgesItself(kind: AlertRuleKind): boolean {
  return kind === 'anomaly' || kind === 'composite' || kind === 'forecast' || kind === 'outlier' || kind === 'heartbeat';
}

/** Kinds that run no query: a composite rule reads other rules, a heartbeat rule its check-ins. */
export function withoutQuery(kind: AlertRuleKind): boolean {
  return kind === 'composite' || kind === 'heartbeat';
}

export const DEFAULT_FORECAST: ForecastParams = { lookbackDays: 7, leadSecs: 86_400, model: 'linear' };
export const DEFAULT_OUTLIER: OutlierParams = { tolerance: 3, direction: 'both' };
export const DEFAULT_HEARTBEAT: HeartbeatParams = { periodSecs: 3_600, graceSecs: 300 };

export const FORECAST_LOOKBACK_DAYS = [1, 3, 7, 14, 30] as const;
export const FORECAST_LEAD_SECS = [3_600, 21_600, 43_200, 86_400, 172_800, 259_200, 604_800] as const;

export function defaultComposite(): CompositeQuery {
  return { expression: 'A && B', rules: { A: '', B: '' }, mergeChildren: false };
}

export const LETTERS = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ'.split('');
export const MAX_COMPOSITE_RULES = 10;

/** The letters an expression uses, in order, once each. */
export function expressionLetters(expression: string): string[] {
  const seen: string[] = [];
  for (const match of expression.toUpperCase().matchAll(/[A-Z]/g)) {
    if (!seen.includes(match[0])) seen.push(match[0]);
  }
  return seen;
}

export type CompositeProblem =
  | { kind: 'empty' }
  | { kind: 'characters' }
  | { kind: 'letters'; letters: string[] }
  | { kind: 'unused'; letters: string[] };

/**
 * What keeps a composite from saving, checked as it is typed: only letters, &&, ||, ! and brackets;
 * every letter used names a rule, every rule named is used. The server parses it for good.
 */
export function compositeProblem(composite: CompositeQuery): CompositeProblem | null {
  const expression = composite.expression.trim();
  if (!expression) return { kind: 'empty' };

  // Letters stand alone; between them only &&, ||, ! and brackets.
  const rest = expression.replace(/&&|\|\||[!()\s]/g, ' ');
  if (!/^[A-Za-z ]*$/.test(rest) || /[A-Za-z]{2,}/.test(rest)) return { kind: 'characters' };

  const used = expressionLetters(expression);
  const unnamed = used.filter((letter) => !composite.rules[letter]);
  if (unnamed.length > 0) return { kind: 'letters', letters: unnamed };

  const unused = Object.keys(composite.rules).filter((letter) => !used.includes(letter));
  if (unused.length > 0) return { kind: 'unused', letters: unused };

  return null;
}

/** The next letter not yet naming a rule. */
export function nextLetter(rules: Record<string, string>): string | undefined {
  return LETTERS.find((letter) => !(letter in rules));
}

/** The kind-specific parts a rule is edited with, or a new rule's. */
export function kindParamsOf(rule: AlertRule | undefined) {
  return {
    composite: rule?.query.composite ?? defaultComposite(),
    forecast: rule?.query.forecast ?? DEFAULT_FORECAST,
    outlier: rule?.query.outlier ?? DEFAULT_OUTLIER,
    heartbeat: rule?.query.heartbeat ?? DEFAULT_HEARTBEAT,
  };
}

/** A shell command that checks in at `address`, and one that reports a failure. */
export function heartbeatExamples(address: string): { ok: string; fail: string } {
  return {
    ok: `curl -fsS -m 10 --retry 3 ${address}`,
    fail: `curl -fsS -m 10 --retry 3 "${address}?status=fail"`,
  };
}

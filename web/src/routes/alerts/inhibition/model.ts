import type { InhibitRuleInput } from '@/api/inhibitions';
import type { InhibitRule, LabelMatcher } from '@/types/alerting';

export const DEFAULT_WAIT_SECS = 60;
export const MAX_WAIT_SECS = 600;
export const DEFAULT_LINGER_SECS = 120;
export const MAX_LINGER_SECS = 3600;

/** A new rule says the common thing: on one service, the worse incident holds back the lesser ones. */
export function newRuleInput(): InhibitRuleInput {
  return {
    name: '',
    enabled: true,
    source: { matchers: [], minSeverity: null },
    target: { matchers: [], maxSeverity: null },
    equal: ['service'],
    moreSevereOnly: true,
    waitSecs: DEFAULT_WAIT_SECS,
    lingerSecs: DEFAULT_LINGER_SECS,
    comment: '',
  };
}

export function ruleInput(rule: InhibitRule): InhibitRuleInput {
  return {
    name: rule.name,
    enabled: rule.enabled,
    source: { matchers: rule.source.matchers.map((matcher) => ({ ...matcher })), minSeverity: rule.source.minSeverity ?? null },
    target: { matchers: rule.target.matchers.map((matcher) => ({ ...matcher })), maxSeverity: rule.target.maxSeverity ?? null },
    equal: [...rule.equal],
    moreSevereOnly: rule.moreSevereOnly,
    waitSecs: rule.waitSecs,
    lingerSecs: rule.lingerSecs,
    comment: rule.comment,
  };
}

export const equalFromText = (text: string): string[] => [
  ...new Set(
    text
      .split(',')
      .map((label) => label.trim())
      .filter(Boolean),
  ),
];

const named = (matchers: LabelMatcher[]) => matchers.filter((matcher) => matcher.label.trim() !== '');

/** The rule as it is sent: conditions without a label are rows the form has not filled yet. */
export function cleanInput(input: InhibitRuleInput): InhibitRuleInput {
  return {
    ...input,
    name: input.name.trim(),
    comment: input.comment.trim(),
    source: { ...input.source, matchers: named(input.source.matchers) },
    target: { ...input.target, matchers: named(input.target.matchers) },
  };
}

export type RuleProblem = 'name' | 'no_scope' | 'wait' | 'linger';

/** What keeps a rule from being saved, checked as the server checks it. */
export function ruleProblem(input: InhibitRuleInput): RuleProblem | null {
  const clean = cleanInput(input);

  if (clean.name === '') return 'name';
  // Any incident suppressing every other is never what anyone means.
  if (clean.source.matchers.length === 0 && clean.equal.length === 0) return 'no_scope';
  if (!Number.isInteger(clean.waitSecs) || clean.waitSecs < 0 || clean.waitSecs > MAX_WAIT_SECS) return 'wait';
  if (!Number.isInteger(clean.lingerSecs) || clean.lingerSecs < 0 || clean.lingerSecs > MAX_LINGER_SECS) return 'linger';
  return null;
}

export type RuleStanding = 'holding' | 'on' | 'off';

export function ruleStanding(rule: Pick<InhibitRule, 'enabled' | 'inhibiting'>): RuleStanding {
  if (!rule.enabled) return 'off';
  return rule.inhibiting > 0 ? 'holding' : 'on';
}

import type {
  ActiveAutomationRule,
  AutomationMatchers,
  AutomationRuleInput,
} from '@/api/statusPages';

export interface AutomationRuleOverlap {
  rule: ActiveAutomationRule;
  relation: 'shadows_current' | 'shadowed_by_current';
}

export function findAutomationRuleOverlaps(
  input: AutomationRuleInput,
  rules: ActiveAutomationRule[],
  currentRuleId?: string,
): AutomationRuleOverlap[] {
  const position = input.position ?? rules.length;
  return rules
    .filter((rule) => rule.rule.id !== currentRuleId)
    .filter((rule) => matchersCanOverlap(input.matchers, rule.revision.matchers))
    .map((rule) => ({
      rule,
      relation: rule.rule.position < position ? 'shadows_current' : 'shadowed_by_current',
    }));
}

function matchersCanOverlap(left: AutomationMatchers, right: AutomationMatchers): boolean {
  if (left.source_kind !== right.source_kind) return false;
  if (left.source_id && right.source_id && left.source_id !== right.source_id) return false;

  for (const [key, value] of Object.entries(left.labels)) {
    const other = right.labels[key];
    if (other !== undefined && other !== value) return false;
  }
  return true;
}

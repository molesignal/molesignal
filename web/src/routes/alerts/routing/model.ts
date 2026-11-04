import type {
  ConditionOperator,
  LabelCondition,
  RoutingRule,
  RoutingRuleInput,
  RoutingTarget,
} from '@/api/alertRouting';

export const ROUTING_KEY = ['alert-routing'] as const;
export const ROUTING_RULES_KEY = [...ROUTING_KEY, 'rules'] as const;
export const ROUTING_TEAMS_KEY = [...ROUTING_KEY, 'teams'] as const;

const SYMBOL: Record<ConditionOperator, string> = {
  equals: '=',
  not_equals: '≠',
  in: '∈',
  matches: '~',
};

/** A condition as it reads in the list: `env = prod`, `team ∈ a, b`, `db.system ~ postgres.*`. */
export function conditionText(condition: LabelCondition): string {
  return `${condition.key} ${SYMBOL[condition.operator]} ${condition.values.join(', ')}`;
}

/** The values a condition takes from what was typed: a list for `in`, one value for the rest. */
export function valuesFromText(operator: ConditionOperator, text: string): string[] {
  if (operator === 'in') {
    return text
      .split(/[,\n]/)
      .map((value) => value.trim())
      .filter(Boolean);
  }

  const value = text.trim();
  return value ? [value] : [];
}

/** Labels typed one per line as `key=value`; a line without `=` or a key is left out. */
export function labelsFromText(text: string): Record<string, string> {
  const labels: Record<string, string> = {};

  for (const line of text.split('\n')) {
    const at = line.indexOf('=');
    const key = at > 0 ? line.slice(0, at).trim() : '';

    if (key) labels[key] = line.slice(at + 1).trim();
  }

  return labels;
}

/** The order of the rules after one moves from `from` to `to`. */
export function moved(ids: readonly string[], from: number, to: number): string[] {
  const next = [...ids];
  const [id] = next.splice(from, 1);

  if (id !== undefined) next.splice(Math.max(0, Math.min(to, next.length)), 0, id);
  return next;
}

/** The order after `source` is dropped just before or after `target`; the same order when it does not move. */
export function dropped(
  ids: readonly string[],
  source: string,
  target: string,
  position: 'before' | 'after',
): string[] {
  const rest = ids.filter((id) => id !== source);
  const at = rest.indexOf(target);

  if (source === target || at < 0 || rest.length === ids.length) return [...ids];
  rest.splice(position === 'before' ? at : at + 1, 0, source);
  return rest;
}

/** What a rule is written back as when one thing about it changes. */
export function ruleInput(rule: RoutingRule): RoutingRuleInput {
  return {
    name: rule.name,
    enabled: rule.enabled,
    conditions: rule.conditions,
    minSeverity: rule.minSeverity,
    origins: rule.origins,
    time: rule.time ?? null,
    priorities: rule.priorities ?? [],
    target: rule.target,
  };
}

/** A rule whose policy or team is gone is passed over when incidents are routed. */
export function targetExists(
  target: RoutingTarget,
  policies: ReadonlySet<string>,
  teams: ReadonlySet<string>,
): boolean {
  return target.kind === 'policy' ? policies.has(target.policyId) : teams.has(target.teamId);
}

/** The `default` alias of the escalation policies: the policy named `default`. */
export const DEFAULT_POLICY = 'default';

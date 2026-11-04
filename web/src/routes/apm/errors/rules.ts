import type { ErrorRule, ErrorRuleWrite, ErrorSummary } from '@/api/apm';
import { patternMatches } from '@/api/apmErrorRules';

export const RULE_PATTERNS = ['error_type', 'message', 'frame', 'endpoint'] as const;
export type RulePattern = (typeof RULE_PATTERNS)[number];

/** The field of a rule each pattern is written in. */
export const PATTERN_FIELD = {
  error_type: 'errorType',
  message: 'message',
  frame: 'frame',
  endpoint: 'endpoint',
} as const satisfies Record<RulePattern, keyof ErrorRuleWrite>;

/** A new rule: on the service in view, if any, merging. */
export function emptyRule(service?: { namespace: string; name: string } | null): ErrorRuleWrite {
  return {
    name: '',
    serviceNamespace: service?.namespace ?? '',
    serviceName: service?.name ?? '',
    errorType: '',
    message: '',
    frame: '',
    endpoint: '',
    action: 'merge',
    enabled: true,
  };
}

/** A new rule for errors like this group's: its type, in its service, named after it. */
export function ruleFromGroup(group: ErrorSummary): ErrorRuleWrite {
  return {
    ...emptyRule(group.service),
    name: group.error.errorType,
    errorType: group.error.errorType,
  };
}

export function ruleWrite(rule: ErrorRule): ErrorRuleWrite {
  const { name, serviceNamespace, serviceName, errorType, message, frame, endpoint, action, enabled } = rule;
  return { name, serviceNamespace, serviceName, errorType, message, frame, endpoint, action, enabled };
}

export function hasPattern(rule: ErrorRuleWrite): boolean {
  return RULE_PATTERNS.some((pattern) => rule[PATTERN_FIELD[pattern]].trim() !== '');
}

/** Whether the rule would take errors like the group's, as the server matches an error when it is seen. */
export function ruleMatchesGroup(rule: ErrorRuleWrite, group: ErrorSummary): boolean {
  const name = rule.serviceName.trim();
  const namespace = rule.serviceNamespace.trim();
  const serviceMatches = !name || (name === group.service.name && (!namespace || namespace === group.service.namespace));
  const values: Record<RulePattern, string | null | undefined> = {
    error_type: group.error.errorType,
    message: group.representativeMessage,
    frame: group.error.applicationFrame,
    endpoint: group.error.transactionName,
  };

  return (
    serviceMatches &&
    hasPattern(rule) &&
    RULE_PATTERNS.every((pattern) => {
      const value = rule[PATTERN_FIELD[pattern]].trim();
      return !value || (values[pattern] != null && patternMatches(value, values[pattern]));
    })
  );
}

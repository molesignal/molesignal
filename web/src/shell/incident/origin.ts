import type { Incident } from '@/types/alerting';

/** The `rule_id` every incident opened by hand carries: they count together as one source. */
export const MANUAL_RULE_ID = 'manual';

/** The synthetic check an incident was opened by: its `rule_id` is then the check's, not a rule's. Null for an incident of an alert rule. */
export function syntheticCheckOf(incident: Pick<Incident, 'labels' | 'ruleId'> | null | undefined): string | null {
  return incident?.labels?.['alert.source'] === 'synthetic' ? incident.ruleId : null;
}

/** The alert source an incident was opened by, and the kind of its sender: its `rule_id` is then the source's. */
export function externalSourceOf(
  incident: Pick<Incident, 'labels' | 'ruleId'> | null | undefined,
): { id: string; kind: string } | null {
  return incident?.labels?.['alert.source'] === 'external'
    ? { id: incident.ruleId, kind: incident.labels['external.kind'] ?? '' }
    : null;
}

/** Who opened an incident by hand; null for one a rule, a check or an alert source opened. */
export function manualCreatorOf(
  incident: Pick<Incident, 'labels' | 'annotations'> | null | undefined,
): { userId: string } | null {
  return incident?.labels?.['alert.source'] === 'manual'
    ? { userId: incident.annotations?.created_by ?? '' }
    : null;
}

/** An incident with a rule behind it to read: not one a synthetic check, an alert source or a person opened. */
export function hasRule(incident: Pick<Incident, 'labels' | 'ruleId'> | null | undefined): boolean {
  return (
    Boolean(incident?.ruleId)
    && !syntheticCheckOf(incident)
    && !externalSourceOf(incident)
    && incident?.labels?.['alert.source'] !== 'manual'
  );
}

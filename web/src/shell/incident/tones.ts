import type { PillTone } from '@/shell/chrome';
import type { IncidentStatus, Severity } from '@/types/alerting';

/** The pill a severity is named in, wherever an incident is described. */
export const SEVERITY_TONE: Record<Severity, PillTone> = {
  info: 'blue',
  warning: 'yellow',
  error: 'red',
  critical: 'red',
};

/** The pill an incident's status is named in. */
export const STATUS_TONE: Record<IncidentStatus, PillTone> = {
  open: 'red',
  acknowledged: 'yellow',
  resolved: 'green',
  closed: 'dim',
};

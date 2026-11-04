import type { PillTone } from '@/shell/chrome';
import type { IncidentStatus, Priority, Severity } from '@/types/alerting';

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

/** The pill a priority is named in: the heaviest three stand out, the rest are quiet. */
export const PRIORITY_TONE: Record<Priority, PillTone> = {
  P1: 'red',
  P2: 'yellow',
  P3: 'blue',
  P4: 'dim',
  P5: 'dim',
  P6: 'dim',
  P7: 'dim',
  P8: 'dim',
  P9: 'dim',
};

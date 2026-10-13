import type {
  ComponentStatus,
  IncidentImpact,
  PublicIncidentStatus,
} from '@/api/statusPages';

/**
 * The colours of a public status page.
 *
 * The page is always shown light, on white or near white. Words in a state's
 * colour use the full token, which reads at 4.5:1 or better there; the softer
 * tokens the app uses on dark surfaces do not, so they are kept to dots, bars
 * and tints, never to text.
 */
export { STATUS_TEXT } from '../model';

/** The tint behind the page's verdict, and the colour of its mark. */
export const STATUS_SURFACE: Record<ComponentStatus, { wash: string; mark: string }> = {
  operational: { wash: 'bg-green/[0.06]', mark: 'bg-green/15 text-green' },
  degraded_performance: { wash: 'bg-yellow/[0.07]', mark: 'bg-yellow/15 text-yellow' },
  partial_outage: { wash: 'bg-orange/[0.07]', mark: 'bg-orange/15 text-orange' },
  major_outage: { wash: 'bg-red/[0.06]', mark: 'bg-red/15 text-red' },
  maintenance: { wash: 'bg-blue/[0.06]', mark: 'bg-blue/15 text-blue' },
};

/**
 * A day of an uptime strip or calendar. The days that went well are the ground
 * the others stand out from, so they are the quietest colour on it; still 3:1
 * against the page, as a mark that carries meaning has to be.
 */
export const UPTIME_DAY: Record<ComponentStatus, string> = {
  operational: 'bg-green/75',
  degraded_performance: 'bg-yellow',
  partial_outage: 'bg-orange',
  major_outage: 'bg-red',
  maintenance: 'bg-blue',
};

/** A day nothing was recorded on. */
export const UPTIME_DAY_UNKNOWN = 'bg-bd-2';

export const IMPACT_DOT: Record<IncidentImpact, string> = {
  critical: 'bg-red',
  major: 'bg-orange',
  minor: 'bg-yellow',
  maintenance: 'bg-blue',
};

/** Where an incident stands: the dot beside the word that says it. */
export const INCIDENT_STATUS_DOT: Record<PublicIncidentStatus, string> = {
  investigating: 'bg-red',
  identified: 'bg-orange',
  in_progress: 'bg-yellow',
  monitoring: 'bg-blue',
  resolved: 'bg-green',
  scheduled: 'bg-blue',
  completed: 'bg-green',
  cancelled: 'bg-bd-2',
};

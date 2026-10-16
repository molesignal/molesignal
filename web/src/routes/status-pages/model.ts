import type { TFunction } from 'i18next';

import type {
  ComponentKind,
  ComponentStatus,
  IncidentImpact,
  PublicIncidentStatus,
  StatusPageLanguage,
  StatusPageIncident,
} from '@/api/statusPages';
import type { PillTone } from '@/shell/chrome';

export const COMPONENT_STATUSES: ComponentStatus[] = [
  'operational',
  'degraded_performance',
  'partial_outage',
  'major_outage',
  'maintenance',
];

export const STATUS_PAGE_LANGUAGES: Array<{
  value: StatusPageLanguage;
  label: string;
}> = [
  { value: 'en-us', label: 'English' },
  { value: 'zh-cn', label: '简体中文' },
];

export const DEFAULT_STATUS_PAGE_HISTORY_DAYS = 90;
export const MAX_STATUS_PAGE_HISTORY_DAYS = 365;
const DAY_MICROS = 24 * 60 * 60 * 1_000_000;

export function statusPageHistoryDays(page: { history_days?: number }): number {
  const days = page.history_days ?? DEFAULT_STATUS_PAGE_HISTORY_DAYS;
  return Number.isInteger(days) && days >= 1 && days <= MAX_STATUS_PAGE_HISTORY_DAYS
    ? days
    : DEFAULT_STATUS_PAGE_HISTORY_DAYS;
}

export function isIncidentWithinHistory(
  incident: { started_at: number; ended_at: number | null },
  generatedAt: number,
  historyDays: number,
): boolean {
  const shownAt = incident.ended_at ?? incident.started_at;
  return shownAt >= generatedAt - historyDays * DAY_MICROS;
}

export function statusPageLanguageLabel(language: StatusPageLanguage): string {
  return STATUS_PAGE_LANGUAGES.find((option) => option.value === language)?.label ?? language;
}

export function statusPageLanguages(
  defaultLanguage: StatusPageLanguage,
  languages?: StatusPageLanguage[],
): StatusPageLanguage[] {
  const available = STATUS_PAGE_LANGUAGES.map(({ value }) => value).filter(
    (language) => language === defaultLanguage || languages?.includes(language),
  );
  return [defaultLanguage, ...available.filter((language) => language !== defaultLanguage)];
}

export function resolveStatusPageLanguage(
  defaultLanguage: StatusPageLanguage,
  languages: StatusPageLanguage[] | undefined,
  requestedLanguage: string | null,
): StatusPageLanguage {
  return (
    statusPageLanguages(defaultLanguage, languages).find(
      (language) => language === requestedLanguage,
    ) ?? defaultLanguage
  );
}

export const INCIDENT_STATUSES: PublicIncidentStatus[] = [
  'investigating',
  'identified',
  'in_progress',
  'monitoring',
  'resolved',
];

export const MAINTENANCE_STATUSES: PublicIncidentStatus[] = [
  'scheduled',
  'in_progress',
  'completed',
  'cancelled',
];

export const INCIDENT_IMPACTS: IncidentImpact[] = ['minor', 'major', 'critical'];

export const COMPONENT_TONE: Record<ComponentStatus, PillTone> = {
  operational: 'green',
  degraded_performance: 'yellow',
  partial_outage: 'orange',
  major_outage: 'red',
  maintenance: 'blue',
};

export const INCIDENT_TONE: Record<PublicIncidentStatus, PillTone> = {
  investigating: 'red',
  identified: 'orange',
  in_progress: 'yellow',
  monitoring: 'blue',
  resolved: 'green',
  scheduled: 'blue',
  completed: 'green',
  cancelled: 'dim',
};

export const IMPACT_TONE: Record<IncidentImpact, PillTone> = {
  minor: 'yellow',
  major: 'orange',
  critical: 'red',
  maintenance: 'blue',
};

export const STATUS_DOT: Record<ComponentStatus, string> = {
  operational: 'bg-green',
  degraded_performance: 'bg-yellow',
  partial_outage: 'bg-orange',
  major_outage: 'bg-red',
  maintenance: 'bg-blue',
};

/**
 * A status as coloured words. These are the full tokens, which read at 4.5:1
 * or better on the surfaces they are set on; the softer tokens are for dots,
 * bars and tints.
 */
export const STATUS_TEXT: Record<ComponentStatus, string> = {
  operational: 'text-green',
  degraded_performance: 'text-yellow',
  partial_outage: 'text-orange',
  major_outage: 'text-red',
  maintenance: 'text-blue',
};

export function componentStatusLabel(t: TFunction, status: ComponentStatus): string {
  return t(`component_status.${status}`);
}

/**
 * Where an incident or a piece of maintenance stands, in words. The two share
 * their statuses, but not all their wording: an incident being worked on has a
 * "fix in progress", while planned work is simply in progress.
 */
export function incidentStatusLabel(
  t: TFunction,
  status: PublicIncidentStatus,
  kind: StatusPageIncident['kind'] = 'incident',
): string {
  if (kind === 'maintenance' && status === 'in_progress') return t('maintenance_status.in_progress');

  return t(`incident_status.${status}`);
}

export function impactLabel(t: TFunction, impact: IncidentImpact): string {
  return t(`impact.${impact}`);
}

export function formatMicros(
  micros: number,
  timezone: string,
  language: string,
  includeDate = true,
): string {
  const date = new Date(Math.floor(micros / 1_000));
  return new Intl.DateTimeFormat(language, {
    ...(includeDate ? { year: 'numeric', month: 'short', day: 'numeric' } : {}),
    hour: '2-digit',
    minute: '2-digit',
    timeZone: timezone,
    timeZoneName: 'short',
  }).format(date);
}

export function formatDurationMicros(t: TFunction, startedAt: number, endedAt: number): string {
  const totalMinutes = Math.floor(Math.max(0, endedAt - startedAt) / 60_000_000);
  if (totalMinutes < 1) return t('values.duration_less_minute');
  const hours = Math.floor(totalMinutes / 60);
  const minutes = totalMinutes % 60;
  if (hours > 0 && minutes > 0) {
    return t('values.duration_hours_minutes', { hours, minutes });
  }
  if (hours > 0) return t('values.duration_hours', { count: hours });
  return t('values.duration_minutes', { count: minutes });
}

interface GroupedComponent {
  id: string;
  kind?: ComponentKind;
  group_id?: string | null;
}

export function isComponentGroup(component: Pick<GroupedComponent, 'kind'>): boolean {
  return component.kind === 'group';
}

/** The components that have a status of their own: everything but groups. */
export function namedComponents<T extends GroupedComponent>(components: readonly T[]): T[] {
  return components.filter((component) => !isComponentGroup(component));
}

export interface ComponentTreeEntry<T> {
  item: T;
  /** The components in a group; empty for a component. */
  members: T[];
}

/**
 * The components of a page as it lists them, in the order given: everything
 * at the top level, each group with the components in it. A component whose
 * group is not among them stands on its own.
 */
export function componentTree<T extends GroupedComponent>(components: readonly T[]): ComponentTreeEntry<T>[] {
  const groups = new Set(components.filter(isComponentGroup).map((component) => component.id));

  return components
    .filter((component) => isComponentGroup(component) || !component.group_id || !groups.has(component.group_id))
    .map((item) => ({
      item,
      members: isComponentGroup(item)
        ? components.filter((component) => !isComponentGroup(component) && component.group_id === item.id)
        : [],
    }));
}

/** The status a group shows: the worst among its components. */
export function groupStatus(members: ReadonlyArray<{ status: ComponentStatus }>): ComponentStatus {
  return members.reduce<ComponentStatus>(
    (worst, member) => (COMPONENT_STATUS_RANK[member.status] > COMPONENT_STATUS_RANK[worst] ? member.status : worst),
    'operational',
  );
}

const COMPONENT_STATUS_RANK: Record<ComponentStatus, number> = {
  operational: 0,
  maintenance: 1,
  degraded_performance: 2,
  partial_outage: 3,
  major_outage: 4,
};

export function affectedComponentNames(
  incident: Pick<StatusPageIncident, 'component_ids'>,
  components: Array<{ id: string; name: string }>,
): string[] {
  const names = new Map(components.map((component) => [component.id, component.name]));
  return incident.component_ids.map((id) => names.get(id)).filter((name): name is string => !!name);
}

export function allowedNextStatuses(status: PublicIncidentStatus): PublicIncidentStatus[] {
  const transitions: Record<PublicIncidentStatus, PublicIncidentStatus[]> = {
    investigating: ['investigating', 'identified', 'in_progress', 'monitoring', 'resolved'],
    identified: ['identified', 'in_progress', 'monitoring', 'resolved'],
    in_progress: ['in_progress', 'monitoring', 'resolved'],
    monitoring: ['monitoring', 'in_progress', 'resolved'],
    resolved: [],
    scheduled: ['scheduled', 'in_progress', 'cancelled'],
    completed: [],
    cancelled: [],
  };
  return transitions[status];
}

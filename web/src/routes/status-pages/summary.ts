import type { TFunction } from 'i18next';

import type {
  ComponentStatus,
  IncidentImpact,
  StatusPageComponent,
  StatusPageIncident,
  StatusPageSnapshot,
} from '@/api/statusPages';
import type { SpectrumSegment } from '@/shell/readout/SpectrumBar';

import { COMPONENT_STATUSES, componentStatusLabel } from './model';

/** The colour of a component status wherever it is drawn as a mark: a dot, a bar, a share of a bar. */
export const STATUS_COLOR: Record<ComponentStatus, string> = {
  operational: 'var(--green)',
  degraded_performance: 'var(--yellow)',
  partial_outage: 'var(--orange)',
  major_outage: 'var(--red)',
  maintenance: 'var(--blue)',
};

/** How bad a status is, as the server ranks it when it picks the worst of several. */
const STATUS_RANK: Record<ComponentStatus, number> = {
  operational: 0,
  maintenance: 1,
  degraded_performance: 2,
  partial_outage: 3,
  major_outage: 4,
};

/** The status an event of this impact puts its components in while it is under way. */
const IMPACT_STATUS: Record<IncidentImpact, ComponentStatus> = {
  minor: 'degraded_performance',
  major: 'partial_outage',
  critical: 'major_outage',
  maintenance: 'maintenance',
};

const IMPACT_RANK: Record<IncidentImpact, number> = {
  critical: 3,
  major: 2,
  minor: 1,
  maintenance: 0,
};

export function worstStatus(left: ComponentStatus, right: ComponentStatus): ComponentStatus {
  return STATUS_RANK[right] > STATUS_RANK[left] ? right : left;
}

/**
 * A component as the page's readers see it now. The status kept on the
 * component is its own; while an event that names it is under way, the public
 * page shows the worse of the two, and so does everything that describes what
 * customers see.
 */
export interface SeenComponent {
  component: StatusPageComponent;
  status: ComponentStatus;
}

/** The components the public page lists, in its order, each in the status it is shown in. */
export function seenComponents(snapshot: StatusPageSnapshot): SeenComponent[] {
  // A group shows what its components show; it is not counted on its own.
  return snapshot.components
    .filter((component) =>
      component.kind !== 'group' && component.lifecycle === 'active' && component.visibility === 'enabled')
    .sort((left, right) => left.position - right.position || left.name.localeCompare(right.name))
    .map((component) => ({
      component,
      status: snapshot.active_incidents
        .filter((event) => event.component_ids.includes(component.id))
        .reduce<ComponentStatus>(
          (status, event) => worstStatus(status, IMPACT_STATUS[event.impact]),
          component.status,
        ),
    }));
}

/** How the components divide by status: one bar, the worst last to be missed. */
export function componentSpectrum(t: TFunction, components: readonly SeenComponent[]): SpectrumSegment[] {
  return COMPONENT_STATUSES.map((status) => ({
    key: status,
    label: componentStatusLabel(t, status),
    value: components.filter((seen) => seen.status === status).length,
    color: STATUS_COLOR[status],
  }));
}

/** What is under way, the worst first and among equals the one that began last. */
export function eventsUnderWay(snapshot: StatusPageSnapshot): StatusPageIncident[] {
  return [...snapshot.active_incidents].sort(
    (left, right) => IMPACT_RANK[right.impact] - IMPACT_RANK[left.impact] || right.started_at - left.started_at,
  );
}

/** Maintenance that is announced and has not begun, the soonest first. */
export function maintenanceAhead(snapshot: StatusPageSnapshot): StatusPageIncident[] {
  return [...snapshot.scheduled_maintenance].sort((left, right) => left.started_at - right.started_at);
}

/** The latest thing said about an event, if anything has been. */
export function latestUpdate(event: StatusPageIncident): StatusPageIncident['updates'][number] | undefined {
  return event.updates.reduce<StatusPageIncident['updates'][number] | undefined>(
    (latest, update) => (!latest || update.created_at > latest.created_at ? update : latest),
    undefined,
  );
}

/** Where an event is managed: an incident and a piece of maintenance live under different tabs. */
export function eventPath(pageId: string, event: Pick<StatusPageIncident, 'id' | 'kind'>): string {
  const tab = event.kind === 'maintenance' ? 'maintenance' : 'incidents';

  return `/status-pages/${encodeURIComponent(pageId)}/${tab}/${encodeURIComponent(event.id)}`;
}

/** Where an event that is over is kept: the record of the page. */
export function endedEventPath(pageId: string, eventId: string): string {
  return `/status-pages/${encodeURIComponent(pageId)}/history/${encodeURIComponent(eventId)}`;
}

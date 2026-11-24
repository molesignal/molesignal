import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { type ChangeEntry, type ChangeKind, REPORTED_CHANGE_KINDS } from '@/api/changes';
import { changeSummary } from '@/shell/changes/model';
import { useChanges } from '@/shell/changes/useChanges';
import type { TimeSeriesAnnotation } from '@/viz/timeseries/types';

import type {
  DashboardAnnotation,
  DashboardTimeRange,
} from './schema';
import {
  interpolateRecord,
  type DashboardVariableValues,
} from './variables';

/**
 * Custom annotation definitions can carry static/event-provider results in
 * `query.items`. Provider-backed sources keep the same contract and can be
 * connected without changing panels or visualization plugins.
 */
export function resolveInlineAnnotations(
  definitions: readonly DashboardAnnotation[],
  variables: DashboardVariableValues,
  timeRange: DashboardTimeRange,
): TimeSeriesAnnotation[] {
  return definitions
    .filter((definition) => definition.enabled)
    .flatMap((definition) => {
      const query = interpolateRecord(
        definition.query ?? {},
        variables,
      ) as Record<string, unknown>;
      const items = Array.isArray(query.items) ? query.items : [];
      return items.flatMap((item, index) => {
        if (!item || typeof item !== 'object' || Array.isArray(item)) return [];
        const event = item as Record<string, unknown>;
        const timestamp = numberValue(event.timestamp ?? event.time);
        if (
          timestamp === undefined ||
          timestamp < timeRange.from ||
          timestamp > timeRange.to
        ) {
          return [];
        }
        const endTimestamp = numberValue(event.endTimestamp ?? event.end);
        return [
          {
            id: stringValue(event.id) || `${definition.id}-${index}`,
            timestamp,
            label:
              stringValue(event.label ?? event.title) || definition.name,
            ...(definition.color ? { color: definition.color } : {}),
            ...(endTimestamp !== undefined ? { endTimestamp } : {}),
          },
        ];
      });
    });
}

function stringValue(value: unknown): string {
  return typeof value === 'string' ? value : '';
}

function numberValue(value: unknown): number | undefined {
  return typeof value === 'number' && Number.isFinite(value) ? value : undefined;
}

/** The ink a change is drawn in on a panel when its definition names none. */
function changeColor(change: ChangeEntry): string {
  if (change.status === 'failed') return 'var(--red)';
  if (change.kind === 'annotation') return 'var(--purple)';
  return 'var(--blue)';
}

/** The services a `deployments` definition names, as a list or comma-separated. */
function servicesOf(definition: DashboardAnnotation, variables: DashboardVariableValues): string[] {
  const query = interpolateRecord(definition.query ?? {}, variables) as Record<string, unknown>;
  const raw = Array.isArray(query.services) ? query.services : stringValue(query.services).split(',');

  return raw.map((service) => stringValue(service).trim()).filter(Boolean);
}

/**
 * Everything a dashboard marks on its time series panels: the events its
 * definitions carry, the changes its `deployments` definitions ask for
 * (reported changes and versions APM first saw, of the services they name or
 * of every service), and the annotations people put on this dashboard or on
 * no dashboard. Times are in microseconds, as the panels' are.
 */
export function useDashboardAnnotations(
  dashboard: { id: string; annotations: readonly DashboardAnnotation[] },
  variables: DashboardVariableValues,
  timeRange: DashboardTimeRange,
): TimeSeriesAnnotation[] {
  const { t } = useTranslation('common');
  const deployments = dashboard.annotations.filter((definition) => definition.enabled && definition.source === 'deployments');
  const services = [...new Set(deployments.flatMap((definition) => servicesOf(definition, variables)))].sort();
  // One definition naming no service asks for every service's changes.
  const everyService = deployments.some((definition) => servicesOf(definition, variables).length === 0);
  const kinds: ChangeKind[] = deployments.length > 0 ? [...REPORTED_CHANGE_KINDS, 'version', 'annotation'] : ['annotation'];
  const changes = useChanges({
    from: timeRange.from,
    to: timeRange.to,
    dashboardId: dashboard.id,
    kinds,
    ...(everyService ? {} : { services }),
  });
  const color = deployments.find((definition) => definition.color)?.color;

  return React.useMemo(() => {
    const inline = resolveInlineAnnotations(dashboard.annotations, variables, timeRange);
    const marked = changes.map((change) => ({
      id: `change:${change.id}`,
      timestamp: change.at,
      label: changeSummary(change, t),
      color: change.kind === 'annotation' ? changeColor(change) : (color ?? changeColor(change)),
      ...(change.end !== undefined ? { endTimestamp: change.end } : {}),
    }));

    return [...inline, ...marked];
  }, [changes, color, dashboard.annotations, t, timeRange, variables]);
}


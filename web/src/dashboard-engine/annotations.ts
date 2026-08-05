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

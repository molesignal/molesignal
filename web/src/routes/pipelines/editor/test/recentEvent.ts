import * as incidentsApi from '@/api/incidents';
import { whenStreamExists } from '@/api/missingStream';
import * as queryApi from '@/api/query';
import type { Incident } from '@/types/alerting';

import type { PipelineSignalType } from '../../PipelineGraph/model';

const DAY_MICROS = 24 * 3600 * 1_000_000;

/**
 * One event the stream received in the last day, as an object, to try a
 * pipeline on real data. `null` when the stream holds none, or does not exist
 * yet. Columns the event has no value for are left out.
 */
export async function recentEvent(
  orgId: string,
  stream: string,
  signalType: PipelineSignalType,
): Promise<Record<string, unknown> | null> {
  const now = Date.now() * 1000;
  const result = await whenStreamExists(
    () =>
      queryApi.runQuery({
        orgId: orgId,
        language: 'sql',
        statement: `SELECT * FROM "${stream.replaceAll('"', '""')}" LIMIT 1`,
        timeRange: { start: now - DAY_MICROS, end: now },
        stream: { name: stream, streamType: signalType },
        limit: 1,
      }),
    () => null,
  );
  const row = result?.rows[0];
  if (!result || !row) return null;

  return Object.fromEntries(
    result.columns
      .map((column, index) => [column, stepValue(signalType, column, row[index])] as const)
      .filter(([, value]) => value !== null && value !== undefined),
  );
}

/** A value as a step sees it: a profile's labels are stored as JSON text, and reach steps as an object. */
function stepValue(signalType: PipelineSignalType, column: string, value: unknown): unknown {
  if (signalType !== 'profiles' || column !== 'labels' || typeof value !== 'string') return value;

  try {
    return JSON.parse(value) as unknown;
  } catch {
    return value;
  }
}

/** How far back the incidents offered as samples go. */
const RECENT_INCIDENTS_SECS = 7 * 24 * 3600;

/** How many incidents are offered as samples. */
const RECENT_INCIDENTS = 20;

/** The latest incidents, to pick one as a sample. */
export async function recentIncidents(): Promise<Incident[]> {
  const incidents = await incidentsApi.list({ scope: 'all', windowSecs: RECENT_INCIDENTS_SECS });

  return [...incidents].sort((a, b) => b.createdAt - a.createdAt).slice(0, RECENT_INCIDENTS);
}

/** Where an incident's event came from, as the steps read it: its labels say. */
export function alertSourceOf(incident: Pick<Incident, 'labels'>): string {
  const source = incident.labels['alert.source'];

  if (source) return source;

  return incident.labels['alert.condition'] === 'no_data' ? 'no_data' : 'rule';
}

/** An incident as an alert pipeline's steps see its event, and what they read as `%rule_id` and the like. */
export function alertEventOf(incident: Incident): {
  event: Record<string, unknown>;
  metadata: Record<string, string>;
} {
  return {
    event: {
      summary: incident.summary,
      severity: incident.severity,
      priority: incident.priority ?? null,
      urgency: incident.urgency ?? null,
      labels: incident.labels,
      annotations: incident.annotations,
      affected_services: incident.affectedServices,
    },
    metadata: {
      rule_id: incident.ruleId,
      // The stored incident does not say its rule's name; its title starts with it.
      rule_name: incident.summary,
      source: alertSourceOf(incident),
      event: 'opened',
    },
  };
}

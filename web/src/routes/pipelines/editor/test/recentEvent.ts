import { whenStreamExists } from '@/api/missingStream';
import * as queryApi from '@/api/query';

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
        org_id: orgId,
        language: 'sql',
        statement: `SELECT * FROM "${stream.replaceAll('"', '""')}" LIMIT 1`,
        time_range: { start: now - DAY_MICROS, end: now },
        stream: { name: stream, stream_type: signalType },
        limit: 1,
      }),
    () => null,
  );
  const row = result?.rows[0];
  if (!result || !row) return null;

  return Object.fromEntries(
    result.columns
      .map((column, index) => [column, row[index]] as const)
      .filter(([, value]) => value !== null && value !== undefined),
  );
}

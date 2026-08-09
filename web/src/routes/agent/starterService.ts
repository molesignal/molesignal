import * as queryApi from '@/api/query';
import * as serviceGraphApi from '@/api/serviceGraph';
import * as streamsApi from '@/api/streams';
import type { StreamSummary } from '@/api/streams';
import type { QueryResult } from '@/types/query';

const LOG_SERVICE_FIELDS = [
  'service.name',
  'service_name',
  'service',
  'resource.service.name',
] as const;
const LOG_SEVERITY_FIELDS = [
  'level',
  'severity_text',
  'severity',
  'log.level',
] as const;
const ERROR_LEVELS = [
  'alert',
  'crit',
  'critical',
  'emerg',
  'err',
  'error',
  'fatal',
] as const;
const MAX_LOG_STREAMS = 8;
const MAX_SERVICES_PER_STREAM = 100;

export interface ServiceErrorObservation {
  service: string;
  source: 'logs' | 'traces';
  errorRate: number;
  sampleCount: number;
}

interface SourceSummary {
  errorEstimate: number;
  sampleCount: number;
}

interface ServiceSummary {
  service: string;
  logs?: SourceSummary;
  traces?: SourceSummary;
}

function escapeSqlIdentifier(identifier: string): string {
  return identifier.replace(/"/g, '""');
}

function quoteSqlIdentifier(identifier: string): string {
  return `"${escapeSqlIdentifier(identifier)}"`;
}

function findSchemaField(
  stream: StreamSummary,
  candidates: readonly string[],
): string | null {
  const fields = new Map(
    stream.schema.fields.map((field) => [field.name.toLowerCase(), field.name]),
  );
  for (const candidate of candidates) {
    const field = fields.get(candidate.toLowerCase());
    if (field) return field;
  }
  return null;
}

export function buildLogServiceErrorQuery(stream: StreamSummary): string | null {
  const serviceField = findSchemaField(stream, LOG_SERVICE_FIELDS);
  const severityField = findSchemaField(stream, LOG_SEVERITY_FIELDS);
  if (!serviceField || !severityField) return null;

  const service = quoteSqlIdentifier(serviceField);
  const severity = quoteSqlIdentifier(severityField);
  const table = quoteSqlIdentifier(stream.name);
  const errorLevels = ERROR_LEVELS.map((level) => `'${level}'`).join(', ');

  return `SELECT
  ${service} AS service,
  COUNT(*) AS total_count,
  SUM(
    CASE
      WHEN LOWER(CAST(${severity} AS VARCHAR)) IN (${errorLevels}) THEN 1
      ELSE 0
    END
  ) AS error_count
FROM ${table}
WHERE ${service} IS NOT NULL
  AND CAST(${service} AS VARCHAR) <> ''
GROUP BY ${service}
ORDER BY error_count DESC, total_count DESC
LIMIT ${MAX_SERVICES_PER_STREAM}`;
}

function finiteNumber(value: unknown): number | null {
  if (typeof value === 'number') return Number.isFinite(value) ? value : null;
  if (typeof value === 'bigint') return Number(value);
  if (typeof value !== 'string' || value.trim() === '') return null;
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : null;
}

function resultColumnIndex(result: QueryResult, name: string): number {
  const normalized = name.toLowerCase();
  return result.columns.findIndex((column) => column.toLowerCase() === normalized);
}

export function logObservationsFromResult(
  result: QueryResult,
): ServiceErrorObservation[] {
  const serviceIndex = resultColumnIndex(result, 'service');
  const totalIndex = resultColumnIndex(result, 'total_count');
  const errorIndex = resultColumnIndex(result, 'error_count');
  if (serviceIndex < 0 || totalIndex < 0 || errorIndex < 0) return [];

  return result.rows.flatMap((row) => {
    const service = String(row[serviceIndex] ?? '').trim();
    const totalCount = finiteNumber(row[totalIndex]);
    const rawErrorCount = finiteNumber(row[errorIndex]);
    if (!isUsableServiceName(service) || totalCount === null || totalCount <= 0) {
      return [];
    }
    const errorCount = Math.min(
      totalCount,
      Math.max(0, rawErrorCount ?? 0),
    );
    return [{
      service,
      source: 'logs' as const,
      errorRate: errorCount / totalCount,
      sampleCount: totalCount,
    }];
  });
}

function isUsableServiceName(service: string): boolean {
  const normalized = service.trim().toLowerCase();
  return Boolean(normalized)
    && !['-', 'n/a', 'null', 'unknown', 'unknown_service'].includes(normalized);
}

function signalScore(summary: SourceSummary): number {
  const errorRate = summary.errorEstimate / summary.sampleCount;
  const confidence = 1 - Math.exp(-summary.sampleCount / 20);
  return errorRate * confidence;
}

export function chooseServiceForErrorInvestigation(
  observations: ServiceErrorObservation[],
): string | null {
  const services = new Map<string, ServiceSummary>();

  for (const observation of observations) {
    const service = observation.service.trim();
    const sampleCount = Math.max(0, observation.sampleCount);
    const errorRate = Math.min(1, Math.max(0, observation.errorRate));
    if (
      !isUsableServiceName(service)
      || !Number.isFinite(sampleCount)
      || sampleCount <= 0
      || !Number.isFinite(errorRate)
    ) {
      continue;
    }

    const key = service.toLowerCase();
    const current = services.get(key) ?? { service };
    const source = current[observation.source] ?? {
      errorEstimate: 0,
      sampleCount: 0,
    };
    source.errorEstimate += errorRate * sampleCount;
    source.sampleCount += sampleCount;
    current[observation.source] = source;
    services.set(key, current);
  }

  const ranked = [...services.values()]
    .map((service) => {
      const signals = [
        service.logs && {
          summary: service.logs,
          weight: 0.45,
        },
        service.traces && {
          summary: service.traces,
          weight: 0.55,
        },
      ].filter((signal): signal is { summary: SourceSummary; weight: number } =>
        Boolean(signal),
      );
      const observedErrors = signals.reduce(
        (total, signal) => total + signal.summary.errorEstimate,
        0,
      );
      const weightTotal = signals.reduce((total, signal) => total + signal.weight, 0);
      const baseScore = signals.reduce(
        (total, signal) => total + signalScore(signal.summary) * signal.weight,
        0,
      ) / weightTotal;
      const corroborated = Boolean(
        service.logs?.errorEstimate && service.traces?.errorEstimate,
      );
      return {
        service: service.service,
        observedErrors,
        sampleCount: signals.reduce(
          (total, signal) => total + signal.summary.sampleCount,
          0,
        ),
        score: baseScore * (corroborated ? 1.1 : 1),
      };
    })
    .filter((service) => service.observedErrors > 0)
    .sort((left, right) =>
      right.score - left.score
      || right.observedErrors - left.observedErrors
      || right.sampleCount - left.sampleCount
      || left.service.localeCompare(right.service),
    );

  return ranked[0]?.service ?? null;
}

function runtimeKey(streamType: string, name: string): string {
  return `${streamType}:${name}`;
}

async function loadLogErrorObservations(
  orgId: string,
  startMicros: number,
  endMicros: number,
): Promise<ServiceErrorObservation[]> {
  const windowSecs = Math.max(
    60,
    Math.round((endMicros - startMicros) / 1_000_000),
  );
  const [streams, runtime] = await Promise.all([
    streamsApi.list(200),
    streamsApi.runtimeOverview({ windowSecs, bucketCount: 1 }).catch(() => null),
  ]);
  const runtimeRows = new Map(
    (runtime?.streams ?? []).map((stream) => [
      runtimeKey(stream.stream_type, stream.name),
      stream.rows,
    ]),
  );
  const candidates = streams
    .filter((stream) =>
      stream.type === 'logs'
      && streamsApi.isQueryable(stream)
      && buildLogServiceErrorQuery(stream) !== null,
    )
    .sort((left, right) =>
      (runtimeRows.get(runtimeKey('logs', right.name)) ?? 0)
      - (runtimeRows.get(runtimeKey('logs', left.name)) ?? 0),
    )
    .slice(0, MAX_LOG_STREAMS);

  const results = await Promise.allSettled(
    candidates.map(async (stream) => {
      const statement = buildLogServiceErrorQuery(stream);
      if (!statement) return [];
      const result = await queryApi.runQuery({
        org_id: orgId,
        language: 'sql',
        statement,
        time_range: { start: startMicros, end: endMicros },
        stream: { name: stream.name, stream_type: 'logs' },
        limit: MAX_SERVICES_PER_STREAM,
      });
      return logObservationsFromResult(result);
    }),
  );

  return results.flatMap((result) =>
    result.status === 'fulfilled' ? result.value : [],
  );
}

export async function discoverStarterService({
  orgId,
  nowMs = Date.now(),
  windowSecs = 60 * 60,
}: {
  orgId: string;
  nowMs?: number;
  windowSecs?: number;
}): Promise<string | null> {
  if (!orgId.trim()) return null;

  const startMs = nowMs - windowSecs * 1000;
  const startMicros = startMs * 1000;
  const endMicros = nowMs * 1000;
  const [traceObservations, logObservations] = await Promise.all([
    serviceGraphApi
      .get(new Date(startMs).toISOString(), new Date(nowMs).toISOString())
      .then((topology) =>
        topology.nodes.map((node) => ({
          service: node.name,
          source: 'traces' as const,
          errorRate: node.error_rate,
          sampleCount: node.span_count,
        })),
      )
      .catch(() => []),
    loadLogErrorObservations(orgId, startMicros, endMicros).catch(() => []),
  ]);

  return chooseServiceForErrorInvestigation([
    ...traceObservations,
    ...logObservations,
  ]);
}

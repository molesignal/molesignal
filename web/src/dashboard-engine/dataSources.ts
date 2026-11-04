import * as profilesApi from '@/api/profiles';
import * as queryApi from '@/api/query';
import { PROMQL_RANGE_ROWS, promqlStepMicros } from '@/lib/promqlRange';
import type { QueryRequest } from '@/types/query';

import { queryResultToDataFrames, rowsToDataFrame } from './dataframe';
import type {
  DashboardTimeRange,
  DataFrame,
  PanelDataSourceType,
  PanelQuery,
} from './schema';
import {
  interpolateRecord,
  interpolateVariables,
  type DashboardVariableValues,
} from './variables';

export interface DataSourceQueryContext {
  orgId: string;
  timeRange: DashboardTimeRange;
  variables: DashboardVariableValues;
  signal?: AbortSignal;
}

export interface DataSourceAdapter {
  type: PanelDataSourceType;
  execute(
    query: PanelQuery,
    context: DataSourceQueryContext,
  ): Promise<DataFrame[]>;
}

class DataSourceAdapterRegistry {
  private readonly adapters = new Map<
    PanelDataSourceType,
    DataSourceAdapter
  >();

  register(adapter: DataSourceAdapter): void {
    this.adapters.set(adapter.type, adapter);
  }

  get(type: PanelDataSourceType): DataSourceAdapter {
    const adapter = this.adapters.get(type);
    if (!adapter) throw new Error(`Unknown dashboard data source: ${type}`);
    return adapter;
  }

  list(): DataSourceAdapter[] {
    return [...this.adapters.values()];
  }
}

export const dataSourceRegistry = new DataSourceAdapterRegistry();

for (const type of ['metrics', 'logs', 'traces', 'sql'] as const) {
  dataSourceRegistry.register({
    type,
    execute: executeQueryApi,
  });
}

dataSourceRegistry.register({
  type: 'profiles',
  execute: executeProfiles,
});

export async function executePanelQuery(
  query: PanelQuery,
  context: DataSourceQueryContext,
): Promise<DataFrame[]> {
  if (!query.enabled) return [];
  return dataSourceRegistry.get(query.dataSourceType).execute(query, context);
}

async function executeQueryApi(
  query: PanelQuery,
  context: DataSourceQueryContext,
): Promise<DataFrame[]> {
  const config = interpolateRecord(
    query.query,
    context.variables,
  ) as Record<string, unknown>;
  const rawExpression =
    stringValue(config.expression) ||
    stringValue(config.statement) ||
    stringValue(config.sql) ||
    stringValue(config.query);
  const statement = interpolateVariables(rawExpression, context.variables);
  if (!statement.trim()) return [];

  const language = resolveLanguage(query.dataSourceType, config);
  const streamName =
    stringValue(config.streamName) ||
    stringValue(config.stream) ||
    (language === 'sql' ? extractSqlStreamName(statement) : '');
  const streamType = resolveStreamType(query.dataSourceType, config);
  // A PromQL panel draws series over the range: it asks for a step, and its
  // limit only caps the rows.
  const promql = language === 'promql';
  const request: QueryRequest = {
    orgId: context.orgId,
    language,
    statement,
    timeRange: {
      start: context.timeRange.from,
      end: context.timeRange.to,
    },
    limit: positiveInteger(config.limit, promql ? PROMQL_RANGE_ROWS : 1000),
  };
  if (promql) {
    request.step = promqlStepMicros(context.timeRange.from, context.timeRange.to);
  }
  if (streamName && streamType) {
    request.stream = { name: streamName, streamType: streamType };
  }
  const result = await queryApi.runQuery(request);
  return queryResultToDataFrames(
    result,
    query.refId,
    query.dataSourceType,
    query.legend,
    query.format,
  );
}

async function executeProfiles(
  query: PanelQuery,
  context: DataSourceQueryContext,
): Promise<DataFrame[]> {
  const config = interpolateRecord(
    query.query,
    context.variables,
  ) as Record<string, unknown>;
  const profiles = await profilesApi.list({
    service: optionalString(config.service),
    type: optionalString(config.profileType ?? config.type),
    from: context.timeRange.from,
    to: context.timeRange.to,
    label: optionalString(config.label),
    traceId: optionalString(config.traceId ?? config.trace_id),
    limit: positiveInteger(config.limit, 500),
  });
  return [
    rowsToDataFrame(
      [
        'timestamp',
        'service',
        'profile_type',
        'total_value',
        'sample_count',
        'duration_nanos',
        'trace_id',
        'span_id',
        'id',
      ],
      profiles.map((profile) => [
        profile.timestamp,
        profile.service,
        profile.profileType,
        profile.totalValue,
        profile.sampleCount,
        profile.durationNanos,
        profile.traceId ?? null,
        profile.spanId ?? null,
        profile.id,
      ]),
      {
        refId: query.refId,
        name: query.legend ?? 'Profiles',
        sourceType: 'profiles',
      },
    ),
  ];
}

function resolveLanguage(
  sourceType: PanelDataSourceType,
  config: Record<string, unknown>,
): QueryRequest['language'] {
  const configured = stringValue(config.language).toLowerCase();
  if (configured === 'promql' || configured === 'sql') return configured;
  return sourceType === 'metrics' ? 'promql' : 'sql';
}

function resolveStreamType(
  sourceType: PanelDataSourceType,
  config: Record<string, unknown>,
): NonNullable<QueryRequest['stream']>['streamType'] | undefined {
  const configured = stringValue(
    config.streamType ?? config.stream_type,
  ).toLowerCase();
  if (
    configured === 'metrics' ||
    configured === 'logs' ||
    configured === 'traces'
  ) {
    return configured;
  }
  if (
    sourceType === 'metrics' ||
    sourceType === 'logs' ||
    sourceType === 'traces'
  ) {
    return sourceType;
  }
  return undefined;
}

function extractSqlStreamName(statement: string): string {
  const match = statement.match(
    /\bfrom\s+((?:"[^"]+"|`[^`]+`|\[[^\]]+\]|[A-Za-z_][\w:.-]*)(?:\s*\.\s*(?:"[^"]+"|`[^`]+`|\[[^\]]+\]|[A-Za-z_][\w:.-]*))*)/i,
  );
  const value = match?.[1];
  if (!value) return '';
  return (
    value
      .split('.')
      .at(-1)
      ?.trim()
      .replace(/^["`[]|["`\]]$/g, '') ?? ''
  );
}

function positiveInteger(value: unknown, fallback: number): number {
  return typeof value === 'number' && Number.isFinite(value)
    ? Math.max(1, Math.round(value))
    : fallback;
}

function stringValue(value: unknown): string {
  return typeof value === 'string' ? value : '';
}

function optionalString(value: unknown): string | undefined {
  const result = stringValue(value).trim();
  return result || undefined;
}

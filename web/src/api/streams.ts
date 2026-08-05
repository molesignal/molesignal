import { http } from '@/lib/http';

export type StreamType = 'logs' | 'metrics' | 'traces' | 'profiles' | 'extend';
export type FieldType = 'bool' | 'int64' | 'float64' | 'utf8' | 'timestamp' | 'json';
export type StreamIndexType = 'none' | 'exact' | 'full_text' | 'bloom' | 'skip';

export interface StreamField {
  name: string;
  data_type: FieldType;
  nullable: boolean;
  indexed: boolean;
  /** 字段级静态加密：写入前用 cipher key 加密、密文落盘；查询用 decrypt(col) 还原。 */
  encrypted?: boolean;
}

export interface FieldIndexRule {
  field: string;
  enabled: boolean;
  index_type: StreamIndexType;
  condition?: string | null;
  sdr_patterns: string[];
}

export interface StreamCondition {
  name: string;
  expression: string;
  enabled: boolean;
  retention_days?: number | null;
}

export interface StreamSettings {
  description?: string | null;
  index_rules: FieldIndexRule[];
  retention_filter?: string | null;
  keep_conditions: StreamCondition[];
  max_query_range_hours?: number | null;
  flatten_level?: number | null;
  use_stream_stats_for_partitioning: boolean;
  store_original_data: boolean;
  enable_distinct_values: boolean;
  /**
   * 是否允许被查询。false 时该 stream 照常采集与保留，但查询端拒绝访问、且从查询选择器隐藏。
   * 用于「源 stream 仅作入口、数据经 pipeline 分流到下游 stream」的场景。默认 true。
   */
  queryable: boolean;
}

export interface StreamSchema {
  fields: StreamField[];
}

export interface StreamRetention {
  days: number;
}

export type StreamRuntimeStatus =
  | 'healthy'
  | 'idle'
  | 'delayed'
  | 'interrupted'
  | 'unused'
  | 'unknown';

export interface StreamRuntimeBucket {
  start_micros: number;
  end_micros: number;
  rows: number;
  stored_bytes: number;
}

export interface StreamRuntime {
  id: string;
  name: string;
  stream_type: Exclude<StreamType, 'extend'>;
  status: StreamRuntimeStatus;
  rows: number;
  stored_bytes: number;
  current_stored_bytes: number;
  first_received_at_micros: number | null;
  last_received_at_micros: number | null;
  stats_available: boolean;
  buckets: StreamRuntimeBucket[];
}

export interface StreamRuntimeOverview {
  generated_at_micros: number;
  window_start_micros: number;
  window_end_micros: number;
  window_secs: number;
  streams: StreamRuntime[];
}

export interface StreamSummary {
  id: string;
  label: string;
  name: string;
  stream_type: StreamType;
  type: StreamType;
  subtitle?: string;
  schema: StreamSchema;
  retention: StreamRetention | null;
  effective_retention: StreamRetention;
  settings: StreamSettings;
  created_at_micros: number;
  updated_at_micros: number;
}

interface StreamResponse {
  id: string;
  name: string;
  stream_type: StreamType;
  schema: StreamSchema;
  retention?: StreamRetention | null;
  effective_retention?: StreamRetention | null;
  settings?: Partial<StreamSettings> | null;
  created_at_micros: number;
  updated_at_micros: number;
}

type StreamListResponse = StreamResponse[] | { items?: unknown[] };

export interface CreateStreamRequest {
  name: string;
  stream_type: StreamType;
  retention_days?: number | null;
  fields?: Array<Pick<StreamField, 'name' | 'data_type' | 'nullable' | 'indexed' | 'encrypted'>>;
  settings?: Partial<StreamSettings>;
}

export interface UpdateStreamSettingsRequest {
  retention_days?: number | null;
  fields?: Array<{
    name: string;
    indexed: boolean;
    index_type: StreamIndexType;
    condition?: string | null;
    sdr_patterns: string[];
  }>;
  settings?: StreamSettings;
}

export function defaultStreamSettings(settings?: Partial<StreamSettings> | null): StreamSettings {
  return {
    description: settings?.description ?? null,
    index_rules: settings?.index_rules ?? [],
    retention_filter: settings?.retention_filter ?? null,
    keep_conditions: settings?.keep_conditions ?? [],
    max_query_range_hours: settings?.max_query_range_hours ?? null,
    flatten_level: settings?.flatten_level ?? null,
    use_stream_stats_for_partitioning: settings?.use_stream_stats_for_partitioning ?? false,
    store_original_data: settings?.store_original_data ?? false,
    enable_distinct_values: settings?.enable_distinct_values ?? true,
    queryable: settings?.queryable ?? true,
  };
}

/** 该 stream 是否可在查询/搜索界面中出现（不可查询的源 stream 应从选择器隐藏）。 */
export function isQueryable(s: StreamSummary): boolean {
  return s.settings.queryable !== false;
}

function adapt(resp: StreamResponse): StreamSummary {
  const settings = defaultStreamSettings(resp.settings);
  const subtitle = settings.description || resp.stream_type;
  const effectiveRetention = resp.effective_retention ?? resp.retention ?? { days: 30 };
  return {
    id: resp.id,
    label: resp.name,
    name: resp.name,
    stream_type: resp.stream_type,
    type: resp.stream_type,
    subtitle,
    schema: resp.schema,
    retention: resp.retention ?? null,
    effective_retention: effectiveRetention,
    settings,
    created_at_micros: resp.created_at_micros,
    updated_at_micros: resp.updated_at_micros,
  };
}

export async function list(limit = 200): Promise<StreamSummary[]> {
  const { data } = await http.get<StreamListResponse>('/streams');
  const items = Array.isArray(data)
    ? data
    : data !== null && typeof data === 'object' && Array.isArray(data.items)
      ? data.items.filter((item): item is StreamResponse => item !== null && typeof item === 'object')
      : [];
  return items.slice(0, limit).map(adapt);
}

export async function get(id: string): Promise<StreamSummary> {
  const { data } = await http.get<StreamResponse>(`/streams/${encodeURIComponent(id)}`);
  return adapt(data);
}

export async function runtimeOverview(params?: {
  windowSecs?: number;
  bucketCount?: number;
}): Promise<StreamRuntimeOverview> {
  const { data } = await http.get<StreamRuntimeOverview>('/streams/runtime', {
    params: {
      window_secs: params?.windowSecs ?? 24 * 60 * 60,
      bucket_count: params?.bucketCount ?? 24,
    },
  });
  return data;
}

export async function create(req: CreateStreamRequest): Promise<StreamSummary> {
  const { data } = await http.post<StreamResponse>('/streams', req);
  return adapt(data);
}

export async function updateSettings(id: string, req: UpdateStreamSettingsRequest): Promise<StreamSummary> {
  const { data } = await http.put<StreamResponse>(`/streams/${encodeURIComponent(id)}/settings`, req);
  return adapt(data);
}

export async function remove(id: string): Promise<void> {
  await http.delete(`/streams/${encodeURIComponent(id)}`);
}

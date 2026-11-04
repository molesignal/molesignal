import type { FieldMaskingAlgorithm } from '@/api/fieldMasking';
import { http } from '@/lib/http';

export type StreamType = 'logs' | 'metrics' | 'traces' | 'profiles' | 'extend';
export type FieldType = 'bool' | 'int64' | 'float64' | 'utf8' | 'timestamp' | 'json';
export type StreamIndexType =
  | 'none'
  | 'exact'
  | 'full_text'
  | 'bloom'
  | 'skip'
  | 'ngram'
  | 'ip'
  | 'key_value';

export interface StreamField {
  name: string;
  dataType: FieldType;
  nullable: boolean;
  indexed: boolean;
  /** 新版 schema 直接持久化的规范索引类型；旧服务响应可能缺失。 */
  indexType?: StreamIndexType;
  /** 字段级静态加密：写入前用 cipher key 加密、密文落盘；查询用 decrypt(col) 还原。 */
  encrypted?: boolean;
}

export interface FieldIndexRule {
  field: string;
  enabled: boolean;
  indexType: StreamIndexType;
}

export interface FieldMaskingOverride {
  field: string;
  /** null explicitly disables masking for this stream field; no entry means inherit. */
  algorithm: FieldMaskingAlgorithm | null;
}

/** 指标流的降采样：早于 `after_days` 天的样本按 `interval_secs` 秒的时间桶聚合。 */
export interface StreamDownsample {
  /** 原始样本保留的天数；0 表示不降采样。 */
  afterDays: number;
  intervalSecs: number;
}

/** 降采样默认的时间桶：一小时。 */
export const DEFAULT_DOWNSAMPLE_INTERVAL_SECS = 3600;

export interface StreamSettings {
  description?: string | null;
  indexRules: FieldIndexRule[];
  /**
   * 写入时第一次出现的字段按名字定索引：按次序第一条名字匹配、类型合适的生效。`*` 是任意一段文字，
   * `?` 是任意一个字符。同名的字段规则优先；都没有时按数据流类型的默认规则。
   */
  indexPatterns?: StreamIndexPattern[];
  /** 一次查询最多覆盖的小时数，更长的时间范围会被拒绝；null 表示不限。 */
  maxQueryRangeHours?: number | null;
  /**
   * 是否允许被查询。false 时该 stream 照常采集与保留，但查询端拒绝访问、且从查询选择器隐藏。
   * 用于「源 stream 仅作入口、数据经 pipeline 分流到下游 stream」的场景。默认 true。
   */
  queryable: boolean;
  fieldMasking?: FieldMaskingOverride[];
  /** 日志与链路：一天结束一小时后，当天不超过合并目标大小的文件合成一个。默认 true。 */
  dailyMerge?: boolean;
  /** 指标：早于若干天的样本按时间桶聚合；默认不降采样。 */
  downsample?: StreamDownsample;
  /** 日志与链路：按一两个文本字段的值把新写入的数据分进几个分片；空表示不分。 */
  partitions?: StreamFieldPartition[];
}

export interface StreamIndexPattern {
  pattern: string;
  indexType: StreamIndexType;
}

/**
 * hash：按值的哈希分进 buckets 片，带这个字段等值条件（= / IN）的查询只读值所在分片的文件。
 * prefix：按值的首字符分片，等值条件与以文字开头的 LIKE（'abc%'）只读首字符所在分片的文件。
 */
export type StreamFieldPartition =
  | { field: string; type: 'hash'; buckets: number }
  | { field: string; type: 'prefix' };

export type StreamPartitionScheme = StreamFieldPartition['type'];

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
  startTime: number;
  endTime: number;
  rows: number;
  storedBytes: number;
}

export interface StreamRuntime {
  id: string;
  name: string;
  streamType: Exclude<StreamType, 'extend'>;
  status: StreamRuntimeStatus;
  rows: number;
  storedBytes: number;
  currentStoredBytes: number;
  totalRows?: number | null;
  collectedBytes?: number | null;
  collectedBytesComplete?: boolean;
  indexBytes?: number | null;
  /** 改了已有字段的索引以后，索引还按旧设置建的文件数：后台补建到它为 0。 */
  staleIndexFiles?: number;
  firstReceivedAt: number | null;
  lastReceivedAt: number | null;
  statsAvailable: boolean;
  buckets: StreamRuntimeBucket[];
}

export interface StreamRuntimeOverview {
  generatedAt: number;
  windowStart: number;
  windowEnd: number;
  windowSecs: number;
  streams: StreamRuntime[];
}

export interface StreamSummary {
  id: string;
  label: string;
  name: string;
  streamType: StreamType;
  type: StreamType;
  subtitle?: string;
  schema: StreamSchema;
  retention: StreamRetention | null;
  effectiveRetention: StreamRetention;
  settings: StreamSettings;
  createdAt: number;
  updatedAt: number;
}

interface StreamResponse {
  id: string;
  name: string;
  streamType: StreamType;
  schema: StreamSchema;
  retention?: StreamRetention | null;
  effectiveRetention?: StreamRetention | null;
  settings?: Partial<StreamSettings> | null;
  createdAt: number;
  updatedAt: number;
}

type StreamListResponse = StreamResponse[] | { items?: unknown[] };

export interface CreateStreamRequest {
  name: string;
  streamType: StreamType;
  retentionDays?: number | null;
  fields?: Array<Pick<StreamField, 'name' | 'dataType' | 'nullable' | 'indexed' | 'indexType' | 'encrypted'>>;
  settings?: Partial<StreamSettings>;
}

export interface UpdateStreamSettingsRequest {
  retentionDays?: number | null;
  fields?: Array<{
    name: string;
    indexed: boolean;
    indexType: StreamIndexType;
  }>;
  settings?: StreamSettings;
}

export function defaultStreamSettings(settings?: Partial<StreamSettings> | null): StreamSettings {
  return {
    description: settings?.description ?? null,
    indexRules: settings?.indexRules ?? [],
    indexPatterns: settings?.indexPatterns ?? [],
    maxQueryRangeHours: settings?.maxQueryRangeHours ?? null,
    queryable: settings?.queryable ?? true,
    fieldMasking: settings?.fieldMasking ?? [],
    dailyMerge: settings?.dailyMerge ?? true,
    downsample: settings?.downsample ?? {
      afterDays: 0,
      intervalSecs: DEFAULT_DOWNSAMPLE_INTERVAL_SECS,
    },
    partitions: settings?.partitions ?? [],
  };
}

/** 该 stream 是否可在查询/搜索界面中出现（不可查询的源 stream 应从选择器隐藏）。 */
export function isQueryable(s: StreamSummary): boolean {
  return s.settings.queryable !== false;
}

function adapt(resp: StreamResponse): StreamSummary {
  const settings = defaultStreamSettings(resp.settings);
  const subtitle = settings.description || resp.streamType;
  const effectiveRetention = resp.effectiveRetention ?? resp.retention ?? { days: 30 };
  return {
    id: resp.id,
    label: resp.name,
    name: resp.name,
    streamType: resp.streamType,
    type: resp.streamType,
    subtitle,
    schema: resp.schema,
    retention: resp.retention ?? null,
    effectiveRetention: effectiveRetention,
    settings,
    createdAt: resp.createdAt,
    updatedAt: resp.updatedAt,
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
      windowSecs: params?.windowSecs ?? 24 * 60 * 60,
      bucketCount: params?.bucketCount ?? 24,
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

import type { TFunction } from 'i18next';

import * as streamsApi from '@/api/streams';

import { toFieldDrafts, type FieldDraft } from './fieldEditor/model';

/** 数据保留天数的上限，与后端一致。 */
export const MAX_RETENTION_DAYS = 3650;

/** 一次查询最多覆盖的小时数的上限：十年。 */
const MAX_QUERY_RANGE_HOURS = MAX_RETENTION_DAYS * 24;

/** 降采样可选的时间桶，单位秒：都整除一天。 */
export const DOWNSAMPLE_INTERVALS = [60, 300, 900, 3600, 86400] as const;

/** 打开降采样时默认保留原始样本的天数。 */
const DEFAULT_DOWNSAMPLE_AFTER_DAYS = 7;

/** 按哈希分片时可选的分片数，与后端的上限一致。 */
export const PARTITION_BUCKETS = [2, 4, 8, 16, 32, 64] as const;

/** 按首字符分片的分片数：数字、字母（不分大小写）各一片，其余一片。 */
export const PREFIX_SHARDS = 37;

/** 分片字段最多几个、各字段分片数之积的上限，与后端一致。 */
export const MAX_PARTITION_FIELDS = 2;
export const MAX_PARTITION_SHARDS = 256;

/** 新加一个按哈希分片的字段时，分片数至多取这么多。 */
const DEFAULT_PARTITION_BUCKETS = 8;

/** 新字段的索引模式最多几条、每条最长几个字符，与后端一致。 */
export const MAX_INDEX_PATTERNS = 32;
const MAX_INDEX_PATTERN_CHARS = 128;

/** 一个分片字段；分片数只用于按哈希分片。 */
export interface PartitionDraft {
  field: string;
  scheme: streamsApi.StreamPartitionScheme;
  buckets: number;
}

/** 数据流设置页上正在编辑的设置。数字按输入的原样保存，保存时才解析。 */
export interface StreamDraft {
  fields: FieldDraft[];
  description: string;
  retentionDays: string;
  maxQueryRange: string;
  queryable: boolean;
  dailyMerge: boolean;
  /** 指标：开着时早于 `downsampleAfterDays` 天的样本按 `downsampleInterval` 秒聚合。 */
  downsample: boolean;
  downsampleAfterDays: string;
  downsampleInterval: number;
  /** 日志与链路：按这些字段分片，最多两个；没有时不分。 */
  partitions: PartitionDraft[];
  /** 写入时第一次出现的字段按名字定索引，按次序第一条匹配的生效。 */
  indexPatterns: streamsApi.StreamIndexPattern[];
}

export function toDraft(stream: streamsApi.StreamSummary): StreamDraft {
  const downsample = stream.settings.downsample;
  const afterDays = downsample?.afterDays ?? 0;

  return {
    fields: toFieldDrafts(stream),
    description: stream.settings.description ?? '',
    retentionDays: stream.retention ? String(stream.retention.days) : '',
    maxQueryRange: stream.settings.maxQueryRangeHours?.toString() ?? '',
    queryable: stream.settings.queryable,
    dailyMerge: stream.settings.dailyMerge ?? true,
    downsample: afterDays > 0,
    downsampleAfterDays: String(afterDays > 0 ? afterDays : DEFAULT_DOWNSAMPLE_AFTER_DAYS),
    downsampleInterval:
      downsample?.intervalSecs ?? streamsApi.DEFAULT_DOWNSAMPLE_INTERVAL_SECS,
    partitions: (stream.settings.partitions ?? []).map((partition) => ({
      field: partition.field,
      scheme: partition.type,
      buckets: partition.type === 'hash' ? partition.buckets : DEFAULT_PARTITION_BUCKETS,
    })),
    indexPatterns: stream.settings.indexPatterns ?? [],
  };
}

/** 一个分片字段分出的分片数。 */
export function partitionShards(partition: PartitionDraft): number {
  return partition.scheme === 'prefix' ? PREFIX_SHARDS : partition.buckets;
}

/** 分片总数：各字段分片数之积。 */
export function totalShards(partitions: readonly PartitionDraft[]): number {
  return partitions.reduce((total, partition) => total * partitionShards(partition), 1);
}

/** 在 `others` 之外再按 `field` 分片：按哈希，分片数取上限还容得下的，至多默认值。 */
export function newPartition(field: string, others: readonly PartitionDraft[]): PartitionDraft {
  const room = Math.min(MAX_PARTITION_SHARDS / totalShards(others), DEFAULT_PARTITION_BUCKETS);
  const buckets = [...PARTITION_BUCKETS].reverse().find((count) => count <= room);
  return { field, scheme: 'hash', buckets: buckets ?? PARTITION_BUCKETS[0] };
}

/** 草稿保存出去的分片字段。 */
export function partitionsForDraft(draft: StreamDraft): streamsApi.StreamFieldPartition[] {
  return draft.partitions
    .filter((partition) => partition.field)
    .map((partition) =>
      partition.scheme === 'prefix'
        ? { field: partition.field, type: 'prefix' }
        : { field: partition.field, type: 'hash', buckets: partition.buckets },
    );
}


/** 输入框里的非负数；空着或不是数时为 `null`。 */
export function parseOptionalNumber(value: string): number | null {
  const trimmed = value.trim();
  if (!trimmed) return null;
  const number = Number(trimmed);
  return Number.isFinite(number) && number >= 0 ? number : null;
}

export function payloadForDraft(draft: StreamDraft): streamsApi.UpdateStreamSettingsRequest {
  const range = parseOptionalNumber(draft.maxQueryRange);

  const settings: streamsApi.StreamSettings = {
    description: draft.description.trim() || null,
    indexRules: draft.fields.map((field) => ({
      field: field.name,
      enabled: field.indexed && field.indexType !== 'none',
      indexType: field.indexType,
    })),
    // 0 与空着一样：不限。
    maxQueryRangeHours: range ? range : null,
    queryable: draft.queryable,
    dailyMerge: draft.dailyMerge,
    downsample: {
      afterDays: draft.downsample ? parseOptionalNumber(draft.downsampleAfterDays) ?? 0 : 0,
      intervalSecs: draft.downsampleInterval,
    },
    partitions: partitionsForDraft(draft),
    indexPatterns: draft.indexPatterns
      .map((pattern) => ({ ...pattern, pattern: pattern.pattern.trim() }))
      .filter((pattern) => pattern.pattern),
    fieldMasking: draft.fields.flatMap((field) => {
      if (field.masking_mode === 'inherit') return [];
      return [{
        field: field.name,
        algorithm: field.masking_mode === 'custom' ? field.masking_algorithm : null,
      }];
    }),
  };

  return {
    retentionDays: draft.retentionDays.trim() ? Number(draft.retentionDays) : null,
    fields: draft.fields.map((field) => ({
      name: field.name,
      indexed: field.indexed,
      indexType: field.indexType,
    })),
    settings,
  };
}

/** 两份草稿保存出去是否一样。 */
export function draftSignature(draft: StreamDraft | null): string {
  return draft ? JSON.stringify(payloadForDraft(draft)) : '';
}

/** 保存前的检查：不能保存时给出原因。 */
export function validateDraft(draft: StreamDraft, t: TFunction<'streams'>): string | null {
  const wholeIn = (value: string, min: number, max: number) => {
    const number = parseOptionalNumber(value);
    return number != null && Number.isInteger(number) && number >= min && number <= max;
  };

  if (draft.retentionDays.trim() && !wholeIn(draft.retentionDays, 1, MAX_RETENTION_DAYS)) {
    return t('explore.validation.retention_days', { max: MAX_RETENTION_DAYS });
  }

  if (draft.maxQueryRange.trim() && !wholeIn(draft.maxQueryRange, 0, MAX_QUERY_RANGE_HOURS)) {
    return t('explore.validation.max_query_range', { max: MAX_QUERY_RANGE_HOURS });
  }

  if (draft.downsample && !wholeIn(draft.downsampleAfterDays, 1, MAX_RETENTION_DAYS)) {
    return t('explore.validation.downsample_after_days', { max: MAX_RETENTION_DAYS });
  }

  const patterns = draft.indexPatterns.map((pattern) => pattern.pattern.trim()).filter(Boolean);

  if (patterns.some((pattern) => [...pattern].length > MAX_INDEX_PATTERN_CHARS)) {
    return t('explore.validation.index_pattern_length', { max: MAX_INDEX_PATTERN_CHARS });
  }

  if (new Set(patterns).size !== patterns.length) {
    return t('explore.validation.index_pattern_duplicate');
  }

  return null;
}

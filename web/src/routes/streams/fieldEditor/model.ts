import type { FieldMaskingAlgorithm } from '@/api/fieldMasking';
import type * as streamsApi from '@/api/streams';
import { defaultAlgorithm } from '@/features/fieldMasking/AlgorithmEditor';

export type FieldMaskingMode = 'inherit' | 'custom' | 'none';

export type FieldDraft = streamsApi.StreamField & {
  indexType: streamsApi.StreamIndexType;
  masking_mode: FieldMaskingMode;
  masking_algorithm: FieldMaskingAlgorithm;
};

export const INDEX_OPTIONS: Array<{
  value: streamsApi.StreamIndexType;
  labelKey: string;
}> = [
  { value: 'none', labelKey: 'explore.index_options.none' },
  { value: 'full_text', labelKey: 'explore.index_options.full_text' },
  { value: 'exact', labelKey: 'explore.index_options.exact' },
  { value: 'bloom', labelKey: 'explore.index_options.bloom' },
  { value: 'skip', labelKey: 'explore.index_options.skip' },
  { value: 'ngram', labelKey: 'explore.index_options.ngram' },
  { value: 'ip', labelKey: 'explore.index_options.ip' },
  { value: 'key_value', labelKey: 'explore.index_options.key_value' },
  { value: 'range', labelKey: 'explore.index_options.range' },
];

/** 只用于文本字段的索引类型。 */
const TEXT_ONLY_INDEXES: ReadonlySet<streamsApi.StreamIndexType> = new Set(['full_text', 'ngram', 'ip']);

/** 只用于 JSON 字段的索引类型。 */
const JSON_ONLY_INDEXES: ReadonlySet<streamsApi.StreamIndexType> = new Set(['key_value']);

/** Index types for numbers and times only. */
const NUMERIC_ONLY_INDEXES: ReadonlySet<streamsApi.StreamIndexType> = new Set(['range']);
const NUMERIC_TYPES: ReadonlySet<streamsApi.FieldType> = new Set(['int64', 'float64', 'timestamp']);

export function indexOptionsFor(dataType: streamsApi.FieldType) {
  return INDEX_OPTIONS.filter(
    (option) =>
      (dataType === 'utf8' || !TEXT_ONLY_INDEXES.has(option.value)) &&
      (dataType === 'json' || !JSON_ONLY_INDEXES.has(option.value)) &&
      (NUMERIC_TYPES.has(dataType) || !NUMERIC_ONLY_INDEXES.has(option.value)),
  );
}

export function toFieldDrafts(stream: streamsApi.StreamSummary): FieldDraft[] {
  return stream.schema.fields.map((field) => {
    const indexRule = stream.settings.indexRules.find((rule) => rule.field === field.name);
    const masking = stream.streamType === 'metrics'
      ? undefined
      : (stream.settings.fieldMasking ?? []).find((item) => item.field === field.name);
    return {
      ...field,
      indexType: indexRule?.indexType ?? field.indexType,
      masking_mode: !masking ? 'inherit' : masking.algorithm ? 'custom' : 'none',
      masking_algorithm: masking?.algorithm ?? defaultAlgorithm('full'),
    };
  });
}

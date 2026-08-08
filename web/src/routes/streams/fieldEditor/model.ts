import type { FieldMaskingAlgorithm } from '@/api/fieldMasking';
import type * as streamsApi from '@/api/streams';
import { defaultAlgorithm } from '@/features/fieldMasking/AlgorithmEditor';

export type FieldMaskingMode = 'inherit' | 'custom' | 'none';

export type FieldDraft = streamsApi.StreamField & {
  index_type: streamsApi.StreamIndexType;
  condition: string;
  extraction_patterns_text: string;
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
];

export function indexOptionsFor(dataType: streamsApi.FieldType) {
  return INDEX_OPTIONS.filter((option) => dataType === 'utf8' || option.value !== 'full_text');
}

function defaultIndexType(field: streamsApi.StreamField): streamsApi.StreamIndexType {
  if (!field.indexed) return 'none';
  return field.data_type === 'utf8' ? 'full_text' : 'exact';
}

export function toFieldDrafts(stream: streamsApi.StreamSummary): FieldDraft[] {
  return stream.schema.fields.map((field) => {
    const indexRule = stream.settings.index_rules.find((rule) => rule.field === field.name);
    const masking = stream.stream_type === 'metrics'
      ? undefined
      : (stream.settings.field_masking ?? []).find((item) => item.field === field.name);
    return {
      ...field,
      index_type: indexRule?.index_type ?? defaultIndexType(field),
      condition: indexRule?.condition ?? '',
      extraction_patterns_text: indexRule?.sdr_patterns.join('\n') ?? '',
      masking_mode: !masking ? 'inherit' : masking.algorithm ? 'custom' : 'none',
      masking_algorithm: masking?.algorithm ?? defaultAlgorithm('full'),
    };
  });
}

import type { TFunction } from 'i18next';

import type { FieldMaskingAlgorithm } from '@/api/fieldMasking';
import { FormField, FormInput, FormRow, FormSelect } from '@/shell/FormDrawer';

export type FieldMaskingAlgorithmKind = FieldMaskingAlgorithm['kind'];

export function defaultAlgorithm(kind: FieldMaskingAlgorithmKind): FieldMaskingAlgorithm {
  switch (kind) {
    case 'range':
      return { kind, start: 0, end: 4, replacement: '******' };
    case 'inner':
      return { kind, prefix_chars: 2, suffix_chars: 2, replacement: '******' };
    case 'outer':
      return { kind, start: 2, end: 6, replacement: '******' };
    case 'hash':
      return { kind };
    case 'full':
    default:
      return { kind: 'full', replacement: '******' };
  }
}

export function algorithmSummary(algorithm: FieldMaskingAlgorithm, t: TFunction): string {
  switch (algorithm.kind) {
    case 'range':
      return t('field_masking.algorithm_summary.range', {
        start: algorithm.start,
        end: algorithm.end,
      });
    case 'inner':
      return t('field_masking.algorithm_summary.inner', {
        prefix: algorithm.prefix_chars,
        suffix: algorithm.suffix_chars,
      });
    case 'outer':
      return t('field_masking.algorithm_summary.outer', {
        start: algorithm.start,
        end: algorithm.end,
      });
    default:
      return t(`field_masking.algorithms.${algorithm.kind}`);
  }
}

export function AlgorithmEditor({
  value,
  onChange,
  disabled = false,
  t,
}: {
  value: FieldMaskingAlgorithm;
  onChange: (value: FieldMaskingAlgorithm) => void;
  disabled?: boolean;
  t: TFunction;
}) {
  const setNumber = (key: 'start' | 'end' | 'prefix_chars' | 'suffix_chars', raw: string) => {
    if (value.kind === 'full' || value.kind === 'hash') return;
    onChange({ ...value, [key]: Math.max(0, Number.parseInt(raw || '0', 10) || 0) });
  };
  const setReplacement = (replacement: string) => {
    if (value.kind === 'hash') return;
    onChange({ ...value, replacement });
  };

  return (
    <>
      <FormField
        label={t('field_masking.field_algorithm')}
        hint={t(`field_masking.algorithm_hints.${value.kind}`)}
      >
        <FormSelect
          value={value.kind}
          onChange={(kind) => onChange(defaultAlgorithm(kind as FieldMaskingAlgorithmKind))}
          options={(['full', 'range', 'inner', 'outer', 'hash'] as const).map((kind) => ({
            value: kind,
            label: t(`field_masking.algorithms.${kind}`),
          }))}
          disabled={disabled}
        />
      </FormField>

      {value.kind === 'range' || value.kind === 'outer' ? (
        <FormRow>
          <FormField label={t('field_masking.field_start')}>
            <FormInput
              type="number"
              min={0}
              value={value.start}
              onChange={(event) => setNumber('start', event.target.value)}
              disabled={disabled}
            />
          </FormField>
          <FormField label={t('field_masking.field_end')}>
            <FormInput
              type="number"
              min={0}
              value={value.end}
              onChange={(event) => setNumber('end', event.target.value)}
              disabled={disabled}
            />
          </FormField>
        </FormRow>
      ) : null}

      {value.kind === 'inner' ? (
        <FormRow>
          <FormField label={t('field_masking.field_prefix')}>
            <FormInput
              type="number"
              min={0}
              value={value.prefix_chars}
              onChange={(event) => setNumber('prefix_chars', event.target.value)}
              disabled={disabled}
            />
          </FormField>
          <FormField label={t('field_masking.field_suffix')}>
            <FormInput
              type="number"
              min={0}
              value={value.suffix_chars}
              onChange={(event) => setNumber('suffix_chars', event.target.value)}
              disabled={disabled}
            />
          </FormField>
        </FormRow>
      ) : null}

      {value.kind !== 'hash' ? (
        <FormField
          label={t('field_masking.field_replacement')}
          hint={t('field_masking.field_replacement_hint')}
        >
          <FormInput
            value={value.replacement}
            onChange={(event) => setReplacement(event.target.value)}
            disabled={disabled}
          />
        </FormField>
      ) : null}
    </>
  );
}

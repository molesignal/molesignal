import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { uiLabelClass } from '@/shell/chrome';
import { FormField, FormInput, FormSelect } from '@/shell/FormDrawer';
import { Checkbox } from '@/shell/ui/checkbox';

import type { ConvertTarget, MaskMode, ProcessorConfig, SetValueType } from './catalog';
import { Segmented } from '../editor/Segmented';

const FIELD_INPUT = 'font-mono text-xs';

// What goes in a field is code, not copy: the examples read the same in every language.
const EXAMPLE = {
  regex: '^(?P<method>\\w+) (?P<path>\\S+) (?P<status>\\d+)$',
  fieldList: 'password, user.token',
} as const;

/**
 * The form of one processor: the fields it reads and writes and how. A field
 * is named the way it reads in an event; `user.email` is `email` inside `user`.
 */
export function ProcessorFields({
  config,
  onChange,
}: {
  config: ProcessorConfig;
  onChange: (next: ProcessorConfig) => void;
}) {
  const { t } = useTranslation('pipelines');
  const field = (
    label: string,
    value: string,
    set: (value: string) => void,
    options: { placeholder?: string; hint?: string } = {},
  ) => (
    <FormField label={label} {...(options.hint ? { hint: options.hint } : {})}>
      <FormInput
        value={value}
        onChange={(event) => set(event.target.value)}
        placeholder={options.placeholder}
        className={FIELD_INPUT}
        autoComplete="off"
        spellCheck={false}
      />
    </FormField>
  );
  const choice = <T extends string>(
    label: string,
    value: T,
    values: readonly T[],
    set: (value: T) => void,
    prefix: string,
  ) => (
    <div className="flex min-w-0 flex-col gap-1.5">
      <span className={uiLabelClass}>{label}</span>
      <Segmented
        label={label}
        value={value}
        onChange={set}
        options={values.map((item) => ({ value: item, label: t(`${prefix}.${item}`) }))}
      />
    </div>
  );

  switch (config.op) {
    case 'parse_json':
    case 'parse_key_value':
      return (
        <Grid>
          {field(t('processors.source_field'), config.source, (source) => onChange({ ...config, source }), {
            placeholder: 'message',
            hint: t('processors.path_hint'),
          })}
          {field(t('processors.target_field'), config.target, (target) => onChange({ ...config, target }), {
            placeholder: t('processors.target_placeholder'),
            hint: t('processors.target_hint'),
          })}
          <label className="col-span-full flex cursor-pointer items-center gap-2 font-sans text-sm text-tx-1">
            <Checkbox
              checked={config.removeSource}
              onCheckedChange={(checked) => onChange({ ...config, removeSource: checked === true })}
            />
            {t('processors.remove_source')}
          </label>
        </Grid>
      );

    case 'parse_regex':
      return (
        <Grid>
          {field(t('processors.source_field'), config.source, (source) => onChange({ ...config, source }), {
            placeholder: 'message',
            hint: t('processors.path_hint'),
          })}
          <div className="sm:col-span-2">
            {field(t('processors.pattern'), config.pattern, (pattern) => onChange({ ...config, pattern }), {
              placeholder: EXAMPLE.regex,
              hint: t('processors.pattern_hint'),
            })}
          </div>
        </Grid>
      );

    case 'rename':
      return (
        <Grid>
          {field(t('processors.from_field'), config.from, (from) => onChange({ ...config, from }), {
            placeholder: 'msg',
            hint: t('processors.path_hint'),
          })}
          {field(t('processors.to_field'), config.to, (to) => onChange({ ...config, to }), {
            placeholder: 'message',
          })}
        </Grid>
      );

    case 'remove':
      return (
        <Grid>
          <div className="sm:col-span-2">
            <FieldList
              label={t('processors.fields')}
              hint={t('processors.fields_hint')}
              fields={config.fields}
              onChange={(fields) => onChange({ ...config, fields })}
            />
          </div>
        </Grid>
      );

    case 'set':
      return (
        <Grid>
          {field(t('processors.field'), config.field, (name) => onChange({ ...config, field: name }), {
            placeholder: 'environment',
            hint: t('processors.path_hint'),
          })}
          <FormField label={t('processors.value_type')}>
            <FormSelect
              value={config.valueType}
              // A value typed for one kind is rarely a value of another.
              onChange={(next) =>
                onChange({
                  ...config,
                  valueType: next as SetValueType,
                  value: next === 'boolean' ? 'true' : '',
                })
              }
              options={(['string', 'number', 'boolean', 'field'] as const).map((item) => ({
                value: item,
                label: t(`processors.value_types.${item}`),
              }))}
            />
          </FormField>
          {config.valueType === 'boolean'
            ? choice(
                t('processors.value'),
                config.value === 'false' ? 'false' : 'true',
                ['true', 'false'] as const,
                (value) => onChange({ ...config, value }),
                'processors.booleans',
              )
            : field(
                config.valueType === 'field' ? t('processors.copy_from') : t('processors.value'),
                config.value,
                (value) => onChange({ ...config, value }),
                { placeholder: config.valueType === 'number' ? '0' : config.valueType === 'field' ? 'kubernetes.labels.app' : 'production' },
              )}
        </Grid>
      );

    case 'mask':
      return (
        <Grid>
          {field(t('processors.field'), config.field, (name) => onChange({ ...config, field: name }), {
            placeholder: 'user.email',
            hint: t('processors.path_hint'),
          })}
          <div className="sm:col-span-2">
            {choice<MaskMode>(
              t('processors.mask_mode'),
              config.mode,
              ['full', 'last4', 'hash'],
              (mode) => onChange({ ...config, mode }),
              'processors.mask_modes',
            )}
          </div>
        </Grid>
      );

    case 'case':
      return (
        <Grid>
          {field(t('processors.field'), config.field, (name) => onChange({ ...config, field: name }), {
            placeholder: 'level',
            hint: t('processors.path_hint'),
          })}
          {choice(
            t('processors.case'),
            config.to,
            ['lower', 'upper'] as const,
            (to) => onChange({ ...config, to }),
            'processors.cases',
          )}
        </Grid>
      );

    case 'convert':
      return (
        <Grid>
          {field(t('processors.field'), config.field, (name) => onChange({ ...config, field: name }), {
            placeholder: 'status',
            hint: t('processors.path_hint'),
          })}
          <FormField label={t('processors.convert_to')} hint={t('processors.convert_hint')}>
            <FormSelect
              value={config.to}
              onChange={(to) => onChange({ ...config, to: to as ConvertTarget })}
              options={(['integer', 'float', 'boolean', 'string'] as const).map((item) => ({
                value: item,
                label: t(`processors.convert_targets.${item}`),
              }))}
            />
          </FormField>
        </Grid>
      );
  }
}

function Grid({ children }: { children: React.ReactNode }) {
  return <div className="grid gap-x-[16px] gap-y-[12px] sm:grid-cols-2 xl:grid-cols-3">{children}</div>;
}

/**
 * Several field names in one line, separated by commas. The text is kept as
 * typed; reformatting it on every key would move the caret.
 */
function FieldList({
  label,
  hint,
  fields,
  onChange,
}: {
  label: string;
  hint: string;
  fields: readonly string[];
  onChange: (fields: string[]) => void;
}) {
  const [text, setText] = React.useState(() => fields.join(', '));
  const split = (value: string) => value.split(',').map((item) => item.trim());

  // Another step's list arrives as new fields that this text does not spell.
  React.useEffect(() => {
    setText((current) =>
      split(current).filter(Boolean).join('\u0000') === fields.filter(Boolean).join('\u0000')
        ? current
        : fields.join(', '),
    );
  }, [fields]);

  return (
    <FormField label={label} hint={hint}>
      <FormInput
        value={text}
        onChange={(event) => {
          setText(event.target.value);
          onChange(split(event.target.value).filter(Boolean));
        }}
        placeholder={EXAMPLE.fieldList}
        className={FIELD_INPUT}
        autoComplete="off"
        spellCheck={false}
      />
    </FormField>
  );
}

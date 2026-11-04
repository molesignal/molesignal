import { useTranslation } from 'react-i18next';

import { FormField, FormInput, FormSelect } from '@/shell/FormDrawer';

import {
  inputIsMissing,
  type PresetInput,
  type PresetInputs as PresetInputValues,
  type TemplatePreset,
  visibleInputs,
} from '../presets';

/** The preset's own fields, two to a row; the endpoints follow from them. */
export function PresetInputs({
  preset,
  values,
  onChange,
}: {
  preset: TemplatePreset;
  values: PresetInputValues;
  onChange: (values: PresetInputValues) => void;
}) {
  const inputs = visibleInputs(preset, values);
  if (inputs.length === 0) return null;
  return (
    <div className="grid grid-cols-2 gap-3">
      {inputs.map((input) => (
        <PresetInputField
          key={input.key}
          input={input}
          value={values[input.key] ?? ''}
          onChange={(value) => onChange({ ...values, [input.key]: value })}
        />
      ))}
    </div>
  );
}

function PresetInputField({
  input,
  value,
  onChange,
}: {
  input: PresetInput;
  value: string;
  onChange: (value: string) => void;
}) {
  const { t } = useTranslation('settings-admin');
  const label = t(`sso_providers.presets.inputs.${input.label}`);
  const malformed = Boolean(value.trim()) && inputIsMissing(input, value);
  const hint =
    input.hint && !malformed ? t(`sso_providers.presets.inputs.${input.hint}`) : undefined;
  return (
    <FormField label={label} required={!input.optional} {...(hint ? { hint } : {})}>
      {input.options ? (
        <FormSelect
          value={value}
          onChange={onChange}
          options={input.options.map((option) => ({
            value: option.value,
            label: t(`sso_providers.presets.inputs.${option.label}`),
          }))}
          ariaLabel={label}
        />
      ) : (
        <FormInput
          value={value}
          onChange={(event) => onChange(event.target.value)}
          placeholder={input.placeholder}
          aria-invalid={malformed || undefined}
          spellCheck={false}
        />
      )}
      {malformed && (
        <span role="alert" className="text-xs leading-relaxed text-red-soft">
          {t('sso_providers.presets.inputs.malformed')}
        </span>
      )}
    </FormField>
  );
}

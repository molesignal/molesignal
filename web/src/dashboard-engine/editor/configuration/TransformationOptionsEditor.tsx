import { Checkbox } from '@/shell/ui/checkbox';

import {
  EditorField,
  EditorInput,
  EditorNumber,
  EditorSelect,
  EditorTextarea,
  OptionalNumberInput,
} from './controls';
import { StringMapEditor } from './StringMapEditor';
import { useDashboardText } from '../../i18n';
import type { TransformationType } from '../../schema';

const REDUCERS = [
  'last_not_null',
  'last',
  'first_not_null',
  'first',
  'min',
  'max',
  'mean',
  'sum',
  'count',
] as const;

export function TransformationOptionsEditor({
  type,
  value,
  onChange,
}: {
  type: TransformationType;
  value: Record<string, unknown>;
  onChange: (value: Record<string, unknown>) => void;
}) {
  const set = (key: string, nextValue: unknown) =>
    onChange({ ...value, [key]: nextValue });

  if (type === 'filter_fields') {
    return (
      <div className="grid grid-cols-2 gap-2">
        <StringListField
          label="Include fields"
          value={stringArray(value.include)}
          onChange={(include) => set('include', include)}
        />
        <StringListField
          label="Exclude fields"
          value={stringArray(value.exclude)}
          onChange={(exclude) => set('exclude', exclude)}
        />
        <EditorField label="Include regex">
          <EditorInput
            value={stringValue(value.includeRegex)}
            mono
            onChange={(includeRegex) => set('includeRegex', includeRegex)}
          />
        </EditorField>
        <EditorField label="Exclude regex">
          <EditorInput
            value={stringValue(value.excludeRegex)}
            mono
            onChange={(excludeRegex) => set('excludeRegex', excludeRegex)}
          />
        </EditorField>
      </div>
    );
  }

  if (type === 'rename_fields') {
    return (
      <div className="space-y-3">
        <StringMapEditor
          value={stringRecord(value.names ?? value.rename)}
          onChange={(names) => set('names', names)}
          keyLabel="Field"
          valueLabel="New name"
          addLabel="Add rename"
        />
        <div className="grid grid-cols-2 gap-2">
          <EditorField label="Regex pattern">
            <EditorInput
              value={stringValue(value.pattern)}
              mono
              onChange={(pattern) => set('pattern', pattern)}
            />
          </EditorField>
          <EditorField label="Replacement">
            <EditorInput
              value={stringValue(value.replacement)}
              mono
              onChange={(replacement) => set('replacement', replacement)}
            />
          </EditorField>
        </div>
      </div>
    );
  }

  if (type === 'organize_fields') {
    return (
      <div className="space-y-3">
        <div className="grid grid-cols-2 gap-2">
          <StringListField
            label="Field order"
            value={stringArray(value.order)}
            onChange={(order) => set('order', order)}
          />
          <StringListField
            label="Hidden fields"
            value={stringArray(value.exclude ?? value.hidden)}
            onChange={(exclude) => set('exclude', exclude)}
          />
        </div>
        <StringMapEditor
          value={stringRecord(value.rename)}
          onChange={(rename) => set('rename', rename)}
          keyLabel="Field"
          valueLabel="New name"
          addLabel="Add rename"
        />
      </div>
    );
  }

  if (type === 'calculate_field') {
    return (
      <div className="space-y-2">
        <div className="grid grid-cols-2 gap-2">
          <EditorField label="Field name">
            <EditorInput
              value={stringValue(value.alias ?? value.name)}
              onChange={(alias) => set('alias', alias)}
            />
          </EditorField>
          <EditorField label="Operation">
            <EditorSelect
              value={stringValue(value.operation) || 'sum'}
              options={[
                ['sum', 'Add'],
                ['subtract', 'Subtract'],
                ['multiply', 'Multiply'],
                ['divide', 'Divide'],
              ]}
              onChange={(operation) => set('operation', operation)}
            />
          </EditorField>
          <EditorField label="Left field">
            <EditorInput
              value={stringValue(value.left)}
              mono
              onChange={(left) => set('left', left)}
            />
          </EditorField>
          <EditorField label="Right field">
            <EditorInput
              value={stringValue(value.right)}
              mono
              onChange={(right) => set('right', right)}
            />
          </EditorField>
          <EditorField label="Static value">
            <OptionalNumberInput
              value={numberValue(value.value)}
              onChange={(staticValue) => set('value', staticValue)}
            />
          </EditorField>
        </div>
        <EditorField label="Expression">
          <EditorTextarea
            value={stringValue(value.expression)}
            rows={2}
            mono
            placeholder="(${requests} / ${total}) * 100"
            onChange={(expression) => set('expression', expression)}
          />
        </EditorField>
      </div>
    );
  }

  if (type === 'reduce' || type === 'time_series_to_table') {
    const fallback =
      type === 'reduce'
        ? [stringValue(value.reducer) || 'last_not_null']
        : ['last', 'min', 'max', 'mean'];
    const selected = stringArray(value.reducers);
    return (
      <ReducerChoices
        value={selected.length > 0 ? selected : fallback}
        onChange={(reducers) => set('reducers', reducers)}
      />
    );
  }

  if (type === 'group_by') {
    return (
      <div className="grid grid-cols-2 gap-2">
        <StringListField
          label="Group fields"
          value={stringArray(value.fields ?? value.groupBy)}
          onChange={(fields) => set('fields', fields)}
        />
        <EditorField label="Calculation">
          <EditorSelect
            value={stringValue(value.reducer) || 'sum'}
            options={REDUCERS.map((reducer) => [reducer, reducer])}
            onChange={(reducer) => set('reducer', reducer)}
          />
        </EditorField>
      </div>
    );
  }

  if (type === 'sort_by') {
    return (
      <div className="grid grid-cols-2 gap-2">
        <EditorField label="Field">
          <EditorInput
            value={stringValue(value.field)}
            mono
            onChange={(field) => set('field', field)}
          />
        </EditorField>
        <EditorField label="Direction">
          <EditorSelect
            value={value.direction === 'desc' ? 'desc' : 'asc'}
            options={[
              ['asc', 'Ascending'],
              ['desc', 'Descending'],
            ]}
            onChange={(direction) => set('direction', direction)}
          />
        </EditorField>
      </div>
    );
  }

  if (type === 'limit') {
    return (
      <div className="grid grid-cols-2 gap-2">
        <EditorField label="Limit">
          <EditorNumber
            value={numberValue(value.count) ?? 10}
            min={0}
            onChange={(count) => set('count', count)}
          />
        </EditorField>
        <EditorField label="Offset">
          <EditorNumber
            value={numberValue(value.offset) ?? 0}
            min={0}
            onChange={(offset) => set('offset', offset)}
          />
        </EditorField>
      </div>
    );
  }

  if (type === 'join') {
    return (
      <div className="grid grid-cols-2 gap-2">
        <EditorField label="Join field">
          <EditorInput
            value={stringValue(value.field ?? value.on)}
            mono
            onChange={(field) => set('field', field)}
          />
        </EditorField>
        <EditorField label="Join mode">
          <EditorSelect
            value={value.mode === 'inner' ? 'inner' : 'outer'}
            options={[
              ['outer', 'Outer'],
              ['inner', 'Inner'],
            ]}
            onChange={(mode) => set('mode', mode)}
          />
        </EditorField>
      </div>
    );
  }

  if (type === 'rows_to_fields') {
    return (
      <div className="grid grid-cols-2 gap-2">
        <EditorField label="Name field">
          <EditorInput
            value={stringValue(value.nameField)}
            mono
            onChange={(nameField) => set('nameField', nameField)}
          />
        </EditorField>
        <EditorField label="Value field">
          <EditorInput
            value={stringValue(value.valueField)}
            mono
            onChange={(valueField) => set('valueField', valueField)}
          />
        </EditorField>
      </div>
    );
  }

  return <NoOptions />;
}

function StringListField({
  label,
  value,
  onChange,
}: {
  label: string;
  value: string[];
  onChange: (value: string[]) => void;
}) {
  return (
    <EditorField label={label}>
      <EditorInput
        value={value.join(', ')}
        mono
        placeholder="field_a, field_b"
        onChange={(nextValue) => onChange(splitList(nextValue))}
      />
    </EditorField>
  );
}

function ReducerChoices({
  value,
  onChange,
}: {
  value: string[];
  onChange: (value: string[]) => void;
}) {
  const tr = useDashboardText();
  return (
    <fieldset className="rounded-md border border-bd-0 p-2">
      <legend className="px-1 font-sans text-xs font-medium text-tx-3">
        {tr('Calculations')}
      </legend>
      <div className="grid grid-cols-2 gap-2 sm:grid-cols-3">
        {REDUCERS.map((reducer) => (
          <label
            key={reducer}
            className="flex items-center gap-2 rounded-md px-1 py-1 font-sans text-xs text-tx-2"
          >
            <Checkbox
              checked={value.includes(reducer)}
              onCheckedChange={(checked) =>
                onChange(
                  checked
                    ? [...value, reducer]
                    : value.filter((candidate) => candidate !== reducer),
                )
              }
            />
            {tr(reducer.replaceAll('_', ' '))}
          </label>
        ))}
      </div>
    </fieldset>
  );
}

function NoOptions() {
  const tr = useDashboardText();
  return (
    <div className="rounded-md border border-dashed border-bd-1 px-3 py-4 text-center font-sans text-xs text-tx-3">
      {tr('No options for this transformation')}
    </div>
  );
}

function splitList(value: string): string[] {
  return value
    .split(',')
    .map((entry) => entry.trim())
    .filter(Boolean);
}

function stringArray(value: unknown): string[] {
  return Array.isArray(value)
    ? value.filter((entry): entry is string => typeof entry === 'string')
    : [];
}

function stringRecord(value: unknown): Record<string, string> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return {};
  return Object.fromEntries(
    Object.entries(value).flatMap(([key, entry]) =>
      typeof entry === 'string' ? [[key, entry]] : [],
    ),
  );
}

function stringValue(value: unknown): string {
  return typeof value === 'string' ? value : '';
}

function numberValue(value: unknown): number | undefined {
  return typeof value === 'number' && Number.isFinite(value) ? value : undefined;
}

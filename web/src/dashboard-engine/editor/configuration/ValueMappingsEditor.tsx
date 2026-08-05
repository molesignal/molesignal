import { Plus, Trash2 } from 'lucide-react';

import {
  EditorField,
  EditorInput,
  EditorSelect,
  OptionalNumberInput,
} from './controls';
import { useDashboardText } from '../../i18n';
import type { ValueMapping } from '../../schema';

const MAPPING_TYPES: ReadonlyArray<readonly [ValueMapping['type'], string]> = [
  ['value', 'Value'],
  ['range', 'Range'],
  ['regex', 'Regex'],
  ['special', 'Special'],
];

export function ValueMappingsEditor({
  value,
  onChange,
}: {
  value: ValueMapping[];
  onChange: (value: ValueMapping[]) => void;
}) {
  const tr = useDashboardText();
  const setMapping = (index: number, mapping: ValueMapping) =>
    onChange(
      value.map((candidate, currentIndex) =>
        currentIndex === index ? mapping : candidate,
      ),
    );

  return (
    <div className="space-y-2">
      {value.length === 0 && (
        <div className="rounded-md border border-dashed border-bd-1 px-3 py-4 text-center font-sans text-xs text-tx-3">
          {tr('No value mappings')}
        </div>
      )}
      {value.map((mapping, index) => (
        <div
          key={index}
          className="space-y-2 rounded-md border border-bd-0 bg-bg-1 p-2.5"
        >
          <div className="flex items-end gap-2">
            <EditorField label="Mapping type">
              <EditorSelect
                value={mapping.type}
                options={MAPPING_TYPES}
                onChange={(type) =>
                  setMapping(
                    index,
                    createMapping(type as ValueMapping['type'], mapping.result),
                  )
                }
              />
            </EditorField>
            <button
              type="button"
              aria-label={`${tr('Remove mapping')} ${index + 1}`}
              onClick={() =>
                onChange(
                  value.filter((_, currentIndex) => currentIndex !== index),
                )
              }
              className="grid h-8 w-8 shrink-0 place-items-center rounded-md text-tx-3 outline-none hover:bg-bg-2 hover:text-danger focus-visible:bg-bg-2 focus-visible:text-danger"
            >
              <Trash2 className="h-3.5 w-3.5" aria-hidden="true" />
            </button>
          </div>
          <MatchEditor
            mapping={mapping}
            onChange={(nextMapping) => setMapping(index, nextMapping)}
          />
          <div className="grid grid-cols-3 gap-2">
            <EditorField label="Display text">
              <EditorInput
                value={mapping.result.text ?? ''}
                onChange={(text) =>
                  setMapping(index, updateResult(mapping, 'text', text))
                }
              />
            </EditorField>
            <EditorField label="Color">
              <EditorInput
                value={mapping.result.color ?? ''}
                placeholder="var(--accent)"
                onChange={(color) =>
                  setMapping(index, updateResult(mapping, 'color', color))
                }
              />
            </EditorField>
            <EditorField label="Icon">
              <EditorInput
                value={mapping.result.icon ?? ''}
                onChange={(icon) =>
                  setMapping(index, updateResult(mapping, 'icon', icon))
                }
              />
            </EditorField>
          </div>
        </div>
      ))}
      <button
        type="button"
        onClick={() =>
          onChange([
            ...value,
            { type: 'value', value: '', result: { text: '' } },
          ])
        }
        className="inline-flex h-8 items-center gap-1.5 rounded-md border border-bd-1 px-2 font-sans text-xs font-medium text-tx-2 outline-none hover:bg-bg-2 hover:text-tx-1 focus-visible:bg-bg-2 focus-visible:text-tx-1"
      >
        <Plus className="h-3.5 w-3.5" aria-hidden="true" />
        {tr('Add mapping')}
      </button>
    </div>
  );
}

function MatchEditor({
  mapping,
  onChange,
}: {
  mapping: ValueMapping;
  onChange: (mapping: ValueMapping) => void;
}) {
  if (mapping.type === 'value') {
    return (
      <EditorField label="Match value">
        <EditorInput
          value={primitiveText(mapping.value)}
          mono
          onChange={(matchValue) => onChange({ ...mapping, value: matchValue })}
        />
      </EditorField>
    );
  }
  if (mapping.type === 'range') {
    return (
      <div className="grid grid-cols-2 gap-2">
        <EditorField label="From">
          <OptionalNumberInput
            value={mapping.from}
            onChange={(from) => onChange({ ...mapping, from })}
          />
        </EditorField>
        <EditorField label="To">
          <OptionalNumberInput
            value={mapping.to}
            onChange={(to) => onChange({ ...mapping, to })}
          />
        </EditorField>
      </div>
    );
  }
  if (mapping.type === 'regex') {
    return (
      <EditorField label="Regex pattern">
        <EditorInput
          value={mapping.pattern}
          mono
          onChange={(pattern) => onChange({ ...mapping, pattern })}
        />
      </EditorField>
    );
  }
  return (
    <EditorField label="Special value">
      <EditorSelect
        value={mapping.match}
        options={[
          ['null', 'Null'],
          ['nan', 'NaN'],
          ['true', 'True'],
          ['false', 'False'],
          ['empty', 'Empty'],
        ]}
        onChange={(match) =>
          onChange({
            ...mapping,
            match: match as Extract<ValueMapping, { type: 'special' }>['match'],
          })
        }
      />
    </EditorField>
  );
}

function createMapping(
  type: ValueMapping['type'],
  result: ValueMapping['result'],
): ValueMapping {
  if (type === 'range') return { type, result };
  if (type === 'regex') return { type, pattern: '', result };
  if (type === 'special') return { type, match: 'null', result };
  return { type: 'value', value: '', result };
}

function updateResult(
  mapping: ValueMapping,
  key: keyof ValueMapping['result'],
  value: string,
): ValueMapping {
  return {
    ...mapping,
    result: { ...mapping.result, [key]: value || undefined },
  };
}

function primitiveText(value: unknown): string {
  if (
    typeof value === 'string' ||
    typeof value === 'number' ||
    typeof value === 'boolean'
  ) {
    return String(value);
  }
  return '';
}

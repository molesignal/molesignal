import { Plus, Trash2 } from 'lucide-react';

import {
  EditorField,
  EditorInput,
  EditorSelect,
  OptionalNumberInput,
} from './controls';
import { ThresholdsEditor } from './ThresholdsEditor';
import { ValueMappingsEditor } from './ValueMappingsEditor';
import { useDashboardText } from '../../i18n';
import type {
  ColorConfig,
  FieldOverrideProperty,
  ThresholdConfig,
  ValueMapping,
} from '../../schema';

const PROPERTY_OPTIONS = [
  ['displayName', 'Display name'],
  ['unit', 'Unit'],
  ['decimals', 'Decimals'],
  ['min', 'Min'],
  ['max', 'Max'],
  ['softMin', 'Soft min'],
  ['softMax', 'Soft max'],
  ['noValue', 'No-value text'],
  ['color', 'Color'],
  ['thresholds', 'Thresholds'],
  ['mappings', 'Value mappings'],
] as const;

export function OverridePropertiesEditor({
  value,
  onChange,
}: {
  value: FieldOverrideProperty[];
  onChange: (value: FieldOverrideProperty[]) => void;
}) {
  const tr = useDashboardText();
  const updateProperty = (index: number, property: FieldOverrideProperty) =>
    onChange(
      value.map((candidate, currentIndex) =>
        currentIndex === index ? property : candidate,
      ),
    );

  return (
    <div className="space-y-2">
      {value.length === 0 && (
        <div className="rounded-md border border-dashed border-bd-1 px-3 py-4 text-center font-sans text-xs text-tx-3">
          {tr('No override properties')}
        </div>
      )}
      {value.map((property, index) => {
        const options = propertyOptions(property.id);
        return (
          <div
            key={`${property.id}-${index}`}
            className="space-y-2 rounded-md border border-bd-0 bg-bg-1 p-2.5"
          >
            <div className="flex items-end gap-2">
              <EditorField label="Property">
                <EditorSelect
                  value={property.id}
                  options={options}
                  onChange={(id) =>
                    updateProperty(index, { id, value: defaultValue(id) })
                  }
                />
              </EditorField>
              <button
                type="button"
                aria-label={`${tr('Remove property')} ${index + 1}`}
                onClick={() =>
                  onChange(
                    value.filter(
                      (_, currentIndex) => currentIndex !== index,
                    ),
                  )
                }
                className="grid h-8 w-8 shrink-0 place-items-center rounded-md text-tx-3 outline-none hover:bg-bg-2 hover:text-danger focus-visible:bg-bg-2 focus-visible:text-danger"
              >
                <Trash2 className="h-3.5 w-3.5" aria-hidden="true" />
              </button>
            </div>
            <PropertyValueEditor
              property={property}
              onChange={(propertyValue) =>
                updateProperty(index, { ...property, value: propertyValue })
              }
            />
          </div>
        );
      })}
      <button
        type="button"
        onClick={() => {
          const id =
            PROPERTY_OPTIONS.find(
              ([candidate]) =>
                !value.some((property) => property.id === candidate),
            )?.[0] ?? 'displayName';
          onChange([...value, { id, value: defaultValue(id) }]);
        }}
        className="inline-flex h-8 items-center gap-1.5 rounded-md border border-bd-1 px-2 font-sans text-xs font-medium text-tx-2 outline-none hover:bg-bg-2 hover:text-tx-1 focus-visible:bg-bg-2 focus-visible:text-tx-1"
      >
        <Plus className="h-3.5 w-3.5" aria-hidden="true" />
        {tr('Add property')}
      </button>
    </div>
  );
}

function PropertyValueEditor({
  property,
  onChange,
}: {
  property: FieldOverrideProperty;
  onChange: (value: unknown) => void;
}) {
  const tr = useDashboardText();
  if (
    property.id === 'displayName' ||
    property.id === 'unit' ||
    property.id === 'noValue'
  ) {
    return (
      <EditorField label="Value">
        <EditorInput
          value={stringValue(property.value)}
          onChange={onChange}
        />
      </EditorField>
    );
  }
  if (
    property.id === 'decimals' ||
    property.id === 'min' ||
    property.id === 'max' ||
    property.id === 'softMin' ||
    property.id === 'softMax'
  ) {
    return (
      <EditorField label="Value">
        <OptionalNumberInput
          value={numberValue(property.value)}
          onChange={onChange}
        />
      </EditorField>
    );
  }
  if (property.id === 'color') {
    const color = colorValue(property.value);
    return (
      <div className="grid grid-cols-2 gap-2">
        <EditorField label="Color mode">
          <EditorSelect
            value={color.mode}
            options={[
              ['palette', 'Palette'],
              ['fixed', 'Fixed'],
              ['thresholds', 'Thresholds'],
              ['continuous', 'Continuous'],
            ]}
            onChange={(mode) =>
              onChange({ ...color, mode: mode as ColorConfig['mode'] })
            }
          />
        </EditorField>
        <EditorField label="Color value">
          <EditorInput
            value={color.value ?? ''}
            placeholder="var(--accent)"
            onChange={(colorText) =>
              onChange({ ...color, value: colorText || undefined })
            }
          />
        </EditorField>
      </div>
    );
  }
  if (property.id === 'thresholds') {
    return (
      <ThresholdsEditor
        value={thresholdValue(property.value)}
        onChange={onChange}
      />
    );
  }
  if (property.id === 'mappings') {
    return (
      <ValueMappingsEditor
        value={mappingValue(property.value)}
        onChange={onChange}
      />
    );
  }
  if (typeof property.value === 'boolean') {
    return (
      <EditorField label="Value">
        <EditorSelect
          value={String(property.value)}
          options={[
            ['true', 'True'],
            ['false', 'False'],
          ]}
          onChange={(value) => onChange(value === 'true')}
        />
      </EditorField>
    );
  }
  if (typeof property.value === 'number') {
    return (
      <EditorField label="Value">
        <OptionalNumberInput
          value={numberValue(property.value)}
          onChange={onChange}
        />
      </EditorField>
    );
  }
  if (typeof property.value === 'string' || property.value == null) {
    return (
      <EditorField label="Value">
        <EditorInput
          value={stringValue(property.value)}
          onChange={onChange}
        />
      </EditorField>
    );
  }
  return (
    <div className="rounded-md border border-dashed border-bd-1 px-3 py-3 font-sans text-xs leading-5 text-tx-3">
      {tr('Imported property value is preserved')}
    </div>
  );
}

function propertyOptions(
  id: FieldOverrideProperty['id'],
): ReadonlyArray<readonly [string, string]> {
  return PROPERTY_OPTIONS.some(([candidate]) => candidate === id)
    ? PROPERTY_OPTIONS
    : [[id, id], ...PROPERTY_OPTIONS];
}

function defaultValue(id: string): unknown {
  if (id === 'decimals') return 2;
  if (id === 'min' || id === 'max' || id === 'softMin' || id === 'softMax') {
    return 0;
  }
  if (id === 'color') return { mode: 'fixed', value: 'var(--accent)' };
  if (id === 'thresholds') return { mode: 'absolute', steps: [] };
  if (id === 'mappings') return [];
  return '';
}

function colorValue(value: unknown): ColorConfig {
  if (!isRecord(value)) return { mode: 'fixed' };
  const mode = ['palette', 'fixed', 'thresholds', 'continuous'].includes(
    stringValue(value.mode),
  )
    ? (value.mode as ColorConfig['mode'])
    : 'fixed';
  return { ...value, mode } as ColorConfig;
}

function thresholdValue(value: unknown): ThresholdConfig {
  if (!isRecord(value)) return { mode: 'absolute', steps: [] };
  return {
    mode: value.mode === 'percentage' ? 'percentage' : 'absolute',
    steps: Array.isArray(value.steps)
      ? (value.steps.filter(isRecord) as unknown as ThresholdConfig['steps'])
      : [],
  };
}

function mappingValue(value: unknown): ValueMapping[] {
  return Array.isArray(value)
    ? (value.filter(isRecord) as unknown as ValueMapping[])
    : [];
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value && typeof value === 'object' && !Array.isArray(value));
}

function stringValue(value: unknown): string {
  return typeof value === 'string' ? value : '';
}

function numberValue(value: unknown): number | undefined {
  return typeof value === 'number' && Number.isFinite(value) ? value : undefined;
}

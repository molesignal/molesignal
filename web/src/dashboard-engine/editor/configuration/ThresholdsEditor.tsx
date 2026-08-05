import { Plus, Trash2 } from 'lucide-react';

import {
  EditorField,
  EditorInput,
  EditorSelect,
  OptionalNumberInput,
} from './controls';
import { useDashboardText } from '../../i18n';
import type { ThresholdConfig, ThresholdStep } from '../../schema';

export function ThresholdsEditor({
  value,
  onChange,
}: {
  value: ThresholdConfig;
  onChange: (value: ThresholdConfig) => void;
}) {
  const tr = useDashboardText();
  const updateStep = (index: number, patch: Partial<ThresholdStep>) =>
    onChange({
      ...value,
      steps: value.steps.map((step, currentIndex) =>
        currentIndex === index ? { ...step, ...patch } : step,
      ),
    });

  return (
    <div className="space-y-2">
      <EditorField label="Threshold mode">
        <EditorSelect
          value={value.mode}
          options={[
            ['absolute', 'Absolute'],
            ['percentage', 'Percentage'],
          ]}
          onChange={(mode) =>
            onChange({
              ...value,
              mode: mode as ThresholdConfig['mode'],
            })
          }
        />
      </EditorField>
      {value.steps.map((step, index) => (
        <div
          key={index}
          className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)_minmax(0,1fr)_32px] items-end gap-2"
        >
          <EditorField label={index === 0 ? 'Base value' : 'Value'}>
            <OptionalNumberInput
              value={step.value ?? undefined}
              placeholder={index === 0 ? tr('Base') : undefined}
              onChange={(stepValue) =>
                updateStep(index, { value: stepValue ?? null })
              }
            />
          </EditorField>
          <EditorField label="Color">
            <EditorInput
              value={step.color}
              placeholder="var(--accent)"
              onChange={(color) => updateStep(index, { color })}
            />
          </EditorField>
          <EditorField label="Label">
            <EditorInput
              value={step.label ?? ''}
              onChange={(label) =>
                updateStep(index, { label: label || undefined })
              }
            />
          </EditorField>
          <button
            type="button"
            aria-label={`${tr('Remove threshold')} ${index + 1}`}
            onClick={() =>
              onChange({
                ...value,
                steps: value.steps.filter(
                  (_, currentIndex) => currentIndex !== index,
                ),
              })
            }
            className="grid h-8 w-8 place-items-center rounded-md text-tx-3 outline-none hover:bg-bg-2 hover:text-danger focus-visible:bg-bg-2 focus-visible:text-danger"
          >
            <Trash2 className="h-3.5 w-3.5" aria-hidden="true" />
          </button>
        </div>
      ))}
      <button
        type="button"
        onClick={() =>
          onChange({
            ...value,
            steps: [
              ...value.steps,
              {
                value: value.steps.length === 0 ? null : 0,
                color: 'var(--accent)',
              },
            ],
          })
        }
        className="inline-flex h-8 items-center gap-1.5 rounded-md border border-bd-1 px-2 font-sans text-xs font-medium text-tx-2 outline-none hover:bg-bg-2 hover:text-tx-1 focus-visible:bg-bg-2 focus-visible:text-tx-1"
      >
        <Plus className="h-3.5 w-3.5" aria-hidden="true" />
        {tr('Add threshold')}
      </button>
    </div>
  );
}

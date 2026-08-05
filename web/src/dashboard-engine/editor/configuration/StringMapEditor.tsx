import { Plus, Trash2 } from 'lucide-react';

import { EditorField, EditorInput } from './controls';
import { useDashboardText } from '../../i18n';

export function StringMapEditor({
  value,
  onChange,
  keyLabel = 'Name',
  valueLabel = 'Value',
  addLabel = 'Add item',
}: {
  value: Record<string, string>;
  onChange: (value: Record<string, string>) => void;
  keyLabel?: string;
  valueLabel?: string;
  addLabel?: string;
}) {
  const tr = useDashboardText();
  const entries = Object.entries(value);

  const updateEntry = (index: number, key: string, entryValue: string) => {
    onChange(
      Object.fromEntries(
        entries.map(([currentKey, currentValue], currentIndex) =>
          currentIndex === index
            ? [key, entryValue]
            : [currentKey, currentValue],
        ),
      ),
    );
  };

  return (
    <div className="space-y-2">
      {entries.map(([key, entryValue], index) => (
        <div
          key={index}
          className="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)_32px] items-end gap-2"
        >
          <EditorField label={keyLabel}>
            <EditorInput
              value={key}
              mono
              onChange={(nextKey) => updateEntry(index, nextKey, entryValue)}
            />
          </EditorField>
          <EditorField label={valueLabel}>
            <EditorInput
              value={entryValue}
              mono
              onChange={(nextValue) => updateEntry(index, key, nextValue)}
            />
          </EditorField>
          <button
            type="button"
            aria-label={`${tr('Remove')} ${key || index + 1}`}
            onClick={() =>
              onChange(
                Object.fromEntries(
                  entries.filter((_, currentIndex) => currentIndex !== index),
                ),
              )
            }
            className="grid h-8 w-8 place-items-center rounded-md text-tx-3 outline-none hover:bg-bg-2 hover:text-danger focus-visible:bg-bg-2 focus-visible:text-danger"
          >
            <Trash2 className="h-3.5 w-3.5" aria-hidden="true" />
          </button>
        </div>
      ))}
      <button
        type="button"
        onClick={() => onChange({ ...value, [nextKey(value)]: '' })}
        className="inline-flex h-8 items-center gap-1.5 rounded-md border border-bd-1 px-2 font-sans text-xs font-medium text-tx-2 outline-none hover:bg-bg-2 hover:text-tx-1 focus-visible:bg-bg-2 focus-visible:text-tx-1"
      >
        <Plus className="h-3.5 w-3.5" aria-hidden="true" />
        {tr(addLabel)}
      </button>
    </div>
  );
}

function nextKey(value: Record<string, string>): string {
  let index = Object.keys(value).length + 1;
  let candidate = `variable_${index}`;
  while (candidate in value) {
    index += 1;
    candidate = `variable_${index}`;
  }
  return candidate;
}

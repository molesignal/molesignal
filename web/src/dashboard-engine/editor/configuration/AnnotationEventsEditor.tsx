import { Plus, Trash2 } from 'lucide-react';

import {
  EditorField,
  EditorInput,
  EditorNumber,
  OptionalNumberInput,
} from './controls';
import { useDashboardText } from '../../i18n';

export function AnnotationEventsEditor({
  value,
  onChange,
}: {
  value: Record<string, unknown>;
  onChange: (value: Record<string, unknown>) => void;
}) {
  const tr = useDashboardText();
  const events = Array.isArray(value.items)
    ? value.items.filter(isRecord)
    : [];

  const setEvents = (items: Array<Record<string, unknown>>) =>
    onChange({ ...value, items });

  return (
    <div className="space-y-2">
      <div className="font-sans text-xs font-medium text-tx-3">
        {tr('Events')}
      </div>
      {events.length === 0 && (
        <div className="rounded-md border border-dashed border-bd-1 px-3 py-4 text-center font-sans text-xs text-tx-3">
          {tr('No annotation events')}
        </div>
      )}
      {events.map((event, index) => (
        <div
          key={`${stringValue(event.id) || 'event'}-${index}`}
          className="space-y-2 rounded-md border border-bd-0 bg-bg-1 p-2.5"
        >
          <div className="flex items-center gap-2">
            <span className="font-sans text-xs font-semibold text-tx-2">
              {tr('Event')} {index + 1}
            </span>
            <button
              type="button"
              aria-label={`${tr('Remove event')} ${index + 1}`}
              onClick={() =>
                setEvents(
                  events.filter((_, currentIndex) => currentIndex !== index),
                )
              }
              className="ml-auto grid h-7 w-7 place-items-center rounded-md text-tx-3 outline-none hover:bg-bg-2 hover:text-danger focus-visible:bg-bg-2 focus-visible:text-danger"
            >
              <Trash2 className="h-3.5 w-3.5" aria-hidden="true" />
            </button>
          </div>
          <div className="grid grid-cols-2 gap-2">
            <EditorField label="Label">
              <EditorInput
                value={stringValue(event.label ?? event.title)}
                onChange={(label) =>
                  setEvents(updateEvent(events, index, { label }))
                }
              />
            </EditorField>
            <EditorField label="Event ID">
              <EditorInput
                value={stringValue(event.id)}
                mono
                onChange={(id) =>
                  setEvents(updateEvent(events, index, { id }))
                }
              />
            </EditorField>
            <EditorField label="Start timestamp (ms)">
              <EditorNumber
                value={numberValue(event.timestamp ?? event.time) ?? 0}
                onChange={(timestamp) =>
                  setEvents(updateEvent(events, index, { timestamp }))
                }
              />
            </EditorField>
            <EditorField label="End timestamp (ms)">
              <OptionalNumberInput
                value={numberValue(event.endTimestamp ?? event.end)}
                onChange={(endTimestamp) =>
                  setEvents(updateEvent(events, index, { endTimestamp }))
                }
              />
            </EditorField>
          </div>
        </div>
      ))}
      <button
        type="button"
        onClick={() =>
          setEvents([
            ...events,
            {
              id: `event-${events.length + 1}`,
              label: `${tr('Event')} ${events.length + 1}`,
              timestamp: Date.now(),
            },
          ])
        }
        className="inline-flex h-8 items-center gap-1.5 rounded-md border border-bd-1 px-2 font-sans text-xs font-medium text-tx-2 outline-none hover:bg-bg-2 hover:text-tx-1 focus-visible:bg-bg-2 focus-visible:text-tx-1"
      >
        <Plus className="h-3.5 w-3.5" aria-hidden="true" />
        {tr('Add event')}
      </button>
    </div>
  );
}

function updateEvent(
  events: Array<Record<string, unknown>>,
  index: number,
  patch: Record<string, unknown>,
): Array<Record<string, unknown>> {
  return events.map((event, currentIndex) =>
    currentIndex === index ? { ...event, ...patch } : event,
  );
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

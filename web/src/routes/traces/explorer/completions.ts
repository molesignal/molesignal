import type { CodeCompletionItem } from '@/shell/codeEditor/types';

import type { TraceFieldDef } from '../fieldQueryModel';
import type { DisplayTrace } from './types';

// The status codes of OpenTelemetry: the values `status_code` can have, whatever is loaded.
const STATUS_CODES = ['OK', 'ERROR', 'UNSET'];

function quoted(value: string): string {
  return `"${value.replace(/\\/g, '\\\\').replace(/"/g, '\\"')}"`;
}

/**
 * What the field query editor offers while typing: the stream's fields, the
 * comparisons, and the values the loaded traces are known by. Only values
 * that are really there are offered; a made-up service name would complete
 * into a query that finds nothing.
 */
export function traceQueryCompletions(fields: TraceFieldDef[], traces: DisplayTrace[]): CodeCompletionItem[] {
  const fieldNames = new Set<string>(fields.map((field) => field.name));
  const values = new Set<string>(STATUS_CODES);

  for (const trace of traces.slice(0, 200)) {
    if (trace.id) values.add(trace.id);
    if (trace.service) values.add(trace.service);
    if (trace.op) values.add(trace.op);
  }

  return [
    ...Array.from(fieldNames).sort().map((label) => ({ label, kind: 'field' as const, detail: 'trace field' })),
    { label: '=', insertText: '= ', kind: 'operator', detail: 'operator' },
    { label: '!=', insertText: '!= ', kind: 'operator', detail: 'operator' },
    { label: '>=', insertText: '>= ', kind: 'operator', detail: 'operator' },
    { label: '<=', insertText: '<= ', kind: 'operator', detail: 'operator' },
    { label: '>', insertText: '> ', kind: 'operator', detail: 'operator' },
    { label: '<', insertText: '< ', kind: 'operator', detail: 'operator' },
    { label: 'contains', insertText: 'contains ', kind: 'operator', detail: 'operator' },
    { label: 'AND', insertText: 'AND ', kind: 'operator', detail: 'operator' },
    { label: 'OR', insertText: 'OR ', kind: 'operator', detail: 'operator' },
    ...Array.from(values).sort().map((value) => {
      const text = quoted(value);
      return { label: text, insertText: text, kind: 'value' as const, detail: 'value' };
    }),
  ];
}

import { Link } from 'react-router-dom';

import type { RumField, RumRow } from '@/api/rum';
import { JsonView } from '@/shell/JsonEditor';
import { SignalReference } from '@/shell/SignalReference';

import { describeCell, type WriteMoment } from '../cells';

/** A value as it is read and searched for: the words of its cell (a moment in the page's time zone), or its JSON. */
export function fieldText(field: RumField | undefined, value: unknown, language: string, moment?: WriteMoment): string {
  if (!field) return typeof value === 'string' ? value : JSON.stringify(value);

  return describeCell(field, value, language, moment)?.text ?? '—';
}

/**
 * A field of a row as the drawer says it: an issue opens on the errors page, a trace on its own
 * page, a JSON document as a document (in words, inside a line); any other value in the words of
 * its cell in the table.
 */
export function RumFieldValue({
  name,
  row,
  field,
  language,
  moment,
  inline = false,
}: {
  name: string;
  row: RumRow;
  field: RumField | undefined;
  language: string;
  moment: WriteMoment;
  /** Said inside a line of words, where a document has no room. */
  inline?: boolean;
}) {
  const value = row[name];

  // The issue the error belongs to opens on the errors page, in its drawer.
  if (name === 'error.issue_id' && typeof value === 'string') {
    const application = row['application.id'];
    const params = new URLSearchParams({ issue: value, ...(typeof application === 'string' && { app: application }) });

    return (
      <Link to={`/rum/errors?${params}`} className="text-[var(--blue-ink)] outline-none hover:underline focus-visible:underline">
        {value}
      </Link>
    );
  }

  if (name === 'resource.trace_id' && typeof value === 'string') {
    return (
      <SignalReference type="trace_id" value={value}>
        {value}
      </SignalReference>
    );
  }

  if (!inline && field?.dataType === 'json' && value !== null && typeof value === 'object') {
    return <JsonView value={value} ariaLabel={name} maxHeight={320} />;
  }

  return <>{fieldText(field, value, language, moment)}</>;
}

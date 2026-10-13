import { ChevronRight } from 'lucide-react';
import * as React from 'react';

import type { SignalReferenceTime } from '@/shell/SignalReference';

import { LogFieldValue, type LogFieldJumpHandler } from './FieldValue';

interface JsonTreeProps {
  obj: Record<string, unknown>;
  /** The whole record, which a value deep inside it is read against. */
  record?: Record<string, unknown>;
  depth?: number;
  path?: string;
  time?: SignalReferenceTime | undefined;
  onJump?: LogFieldJumpHandler | undefined;
}

/** The record as it was stored: every key, nested as it is nested, a value in the colour of its kind. */
export function JsonTree({ obj, record = obj, depth = 0, path = '', time, onJump }: JsonTreeProps) {
  return (
    <div className="font-sans text-xs leading-[1.7]">
      <div>{'{'}</div>
      {Object.entries(obj).map(([key, value], index, entries) => {
        const childPath = path ? `${path}.${key}` : key;

        return (
          <JsonRow
            key={childPath}
            name={key}
            path={childPath}
            value={value}
            last={index === entries.length - 1}
            depth={depth + 1}
            record={record}
            time={time}
            onJump={onJump}
          />
        );
      })}
      <div>{'}'}</div>
    </div>
  );
}

function JsonRow({
  name,
  path,
  value,
  last,
  depth,
  record,
  time,
  onJump,
}: {
  name: string;
  path: string;
  value: unknown;
  last: boolean;
  depth: number;
  record: Record<string, unknown>;
  time?: SignalReferenceTime | undefined;
  onJump?: LogFieldJumpHandler | undefined;
}) {
  const [open, setOpen] = React.useState(depth <= 1);
  const isObject = value !== null && typeof value === 'object' && !Array.isArray(value);
  const isList = Array.isArray(value);
  const indent = depth * 16;
  // The kinds of value keep the colours the field list gives their types.
  const kindColor =
    typeof value === 'string'
      ? 'text-[var(--green-ink)]'
      : typeof value === 'number'
        ? 'text-[var(--orange-ink)]'
        : typeof value === 'boolean'
          ? 'text-[var(--blue-ink)]'
          : 'text-tx-1';

  if (isObject) {
    return (
      <div style={{ paddingLeft: indent }}>
        <button
          type="button"
          aria-expanded={open}
          onClick={() => setOpen(!open)}
          className="flex cursor-pointer items-center gap-1 rounded text-left focus-visible:bg-bg-3"
        >
          <ChevronRight className={`h-3 w-3 shrink-0 text-tx-3 transition-transform ${open ? 'rotate-90' : ''}`} />
          <span className="text-tx-2">{name}:</span>
          {/* Folded, it says there is something inside; open, the braces under it say so. */}
          {!open && <span className="text-tx-3">{'{…}'}</span>}
        </button>
        {open && (
          <JsonTree
            obj={value as Record<string, unknown>}
            record={record}
            depth={depth}
            path={path}
            time={time}
            onJump={onJump}
          />
        )}
      </div>
    );
  }
  if (isList) {
    // A list is written out whole: it has nothing to fold, and its length is plain from it.
    return (
      <div className="flex items-start gap-1.5" style={{ paddingLeft: indent }}>
        <span className="shrink-0 text-tx-2">{name}:</span>
        <span className="min-w-0 whitespace-pre-wrap break-all text-tx-1">
          {JSON.stringify(value)}
          {!last && <span className="text-tx-3">,</span>}
        </span>
      </div>
    );
  }

  return (
    <div className="flex items-start gap-1.5" style={{ paddingLeft: indent }}>
      <span className="shrink-0 text-tx-2">{name}:</span>
      <span className="min-w-0">
        <LogFieldValue field={path} value={value} record={record} time={time} onJump={onJump} className={kindColor} />
        {!last && <span className="text-tx-3">,</span>}
      </span>
    </div>
  );
}

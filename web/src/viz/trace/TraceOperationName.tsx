import { cn } from '@/shell/lib/cn';

import { OPERATION_TONE, type OperationAction, operationAction } from './operationVerb';

export interface ParsedTraceOperation {
  /** The leading word in upper case: an HTTP method, a SQL statement, a Redis or MongoDB command. */
  verb: string;
  action: OperationAction;
  target: string;
}

/** A span name led by a data verb (`GET /cart`, `SELECT orders`, `hgetall session:1`), split at it. */
export function parseTraceOperation(operation: string): ParsedTraceOperation | null {
  const match = operation.match(/^([a-z]+)(?:\s+([\s\S]+))?$/i);
  if (!match?.[1]) return null;
  const action = operationAction(match[1]);
  if (!action) return null;
  return {
    verb: match[1].toUpperCase(),
    action,
    target: match[2] ?? '',
  };
}

/**
 * Trace operation names use the same compact, color-coded prefix treatment as
 * metric kinds (`MetricTypePrefix`), in the same ink tones. HTTP methods, SQL
 * statements and Redis or MongoDB commands lead with their verb, coloured by
 * what it does; other span names (RPC methods, messaging) stay plain text.
 * The verb is a small word in the monospace face, except in a list table
 * (`mono={false}`): there it is a word of the row, in the row's face and size.
 */
export function TraceOperationName({
  operation,
  className,
  mono = true,
}: {
  operation: string;
  className?: string;
  mono?: boolean;
}) {
  const parsed = parseTraceOperation(operation);
  if (!parsed) {
    return (
      <span className={cn('inline-block max-w-full truncate', className)} title={operation}>
        {operation}
      </span>
    );
  }

  return (
    <span
      className={cn('inline-flex max-w-full min-w-0 items-baseline', className)}
      title={operation}
    >
      <span className="sr-only">{operation}</span>
      <span
        aria-hidden="true"
        className={cn(
          'shrink-0 font-semibold',
          mono && 'type-micro font-mono',
          OPERATION_TONE[parsed.action],
        )}
      >
        {parsed.verb}
      </span>
      {parsed.target && (
        <span aria-hidden="true" className="ml-1.5 min-w-0 truncate">
          {parsed.target}
        </span>
      )}
    </span>
  );
}

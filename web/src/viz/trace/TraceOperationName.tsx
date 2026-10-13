import { cn } from '@/shell/lib/cn';

const HTTP_METHODS = [
  'GET',
  'POST',
  'PUT',
  'PATCH',
  'DELETE',
  'HEAD',
  'OPTIONS',
  'CONNECT',
  'TRACE',
] as const;

type HttpMethod = (typeof HTTP_METHODS)[number];

/** The ink tones: a method reads at AA on a row at rest, under the pointer and selected. */
const HTTP_METHOD_TONE: Record<HttpMethod, string> = {
  GET: 'text-[var(--green-ink)]',
  POST: 'text-[var(--blue-ink)]',
  PUT: 'text-[var(--orange-ink)]',
  PATCH: 'text-[var(--orange-ink)]',
  DELETE: 'text-[var(--red-ink)]',
  HEAD: 'text-[var(--purple-ink)]',
  OPTIONS: 'text-[var(--purple-ink)]',
  CONNECT: 'text-[var(--yellow-ink)]',
  TRACE: 'text-[var(--yellow-ink)]',
};

const HTTP_METHOD_SET = new Set<string>(HTTP_METHODS);

export interface ParsedTraceOperation {
  method: HttpMethod;
  target: string;
}

export function parseTraceOperation(operation: string): ParsedTraceOperation | null {
  const match = operation.match(/^([a-z]+)(?:\s+(.+))?$/i);
  if (!match) return null;
  const method = match[1]?.toUpperCase();
  if (!method || !HTTP_METHOD_SET.has(method)) return null;
  return {
    method: method as HttpMethod,
    target: match[2] ?? '',
  };
}

/**
 * Trace operation names use the same compact, color-coded prefix treatment as
 * metric kinds (`MetricTypePrefix`), in the same ink tones. Only standard HTTP
 * methods are highlighted; RPC/database span names remain plain text.
 */
export function TraceOperationName({
  operation,
  className,
}: {
  operation: string;
  className?: string;
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
          'type-micro shrink-0 font-mono font-semibold',
          HTTP_METHOD_TONE[parsed.method],
        )}
      >
        {parsed.method}
      </span>
      {parsed.target && (
        <span aria-hidden="true" className="ml-1.5 min-w-0 truncate">
          {parsed.target}
        </span>
      )}
    </span>
  );
}

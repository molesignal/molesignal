import { CircleAlert } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { Span } from '@/api/web';
import { cn } from '@/shell/lib/cn';

import { formatTraceDurationNs } from '../duration';
import { attributeText } from '../model/span';

/**
 * What happened during the span, in the order it happened, each at its time
 * from the span's start. An exception is said as its type, its message and
 * the stack it was thrown from.
 */
export function SpanEvents({ span, startNs }: { span: Span; startNs: number }) {
  const { t } = useTranslation('traces');

  if (span.events.length === 0) {
    return <p className="px-[6px] py-[8px] font-sans text-xs text-tx-3">{t('view.events.empty')}</p>;
  }

  const events = [...span.events].sort((left, right) => left.tsNs - right.tsNs);

  return (
    <ol className="space-y-1.5" aria-label={t('view.tabs.events')}>
      {events.map((event, index) => {
        const exception = event.name === 'exception';
        const attributes = Object.entries(event.attributes ?? {}).filter(
          ([key]) => !exception || !['exception.type', 'exception.message', 'exception.stacktrace'].includes(key),
        );
        const offset = Math.max(0, event.tsNs - startNs);

        return (
          <li
            key={`${event.name}-${index}`}
            data-testid="trace-span-event"
            className={cn('rounded-md px-[10px] py-[8px]', exception ? 'bg-red-dim' : 'bg-bg-1')}
          >
            <div className="flex items-center gap-1.5">
              {exception && <CircleAlert aria-hidden="true" className="h-3.5 w-3.5 shrink-0 text-[var(--red-ink)]" />}
              <span className={cn('min-w-0 flex-1 truncate font-sans text-xs font-strong', exception ? 'text-[var(--red-ink)]' : 'text-tx-0')}>
                {exception ? attributeText(event.attributes?.['exception.type'] ?? event.name) : event.name}
              </span>
              <span className="shrink-0 font-sans text-xs tabular-nums text-tx-3">+{formatTraceDurationNs(offset)}</span>
            </div>
            {exception && event.attributes?.['exception.message'] !== undefined && (
              <p className="mt-1 whitespace-pre-wrap break-words font-sans text-xs text-tx-0 [overflow-wrap:anywhere]">
                {attributeText(event.attributes['exception.message'])}
              </p>
            )}
            {exception && event.attributes?.['exception.stacktrace'] !== undefined && (
              <pre className="mt-1.5 max-h-[220px] overflow-auto whitespace-pre rounded-sm bg-bg-0 px-2 py-1.5 font-mono text-xs leading-4 text-tx-1">
                {attributeText(event.attributes['exception.stacktrace'])}
              </pre>
            )}
            {attributes.length > 0 && (
              <dl className="mt-1 grid grid-cols-[minmax(0,2fr)_minmax(0,3fr)] gap-x-2 gap-y-0.5">
                {attributes.map(([key, value]) => (
                  <div key={key} className="contents">
                    <dt className="truncate font-sans text-xs text-tx-2" title={key}>{key}</dt>
                    <dd className="truncate font-sans text-xs text-tx-0" title={attributeText(value)}>{attributeText(value)}</dd>
                  </div>
                ))}
              </dl>
            )}
          </li>
        );
      })}
    </ol>
  );
}

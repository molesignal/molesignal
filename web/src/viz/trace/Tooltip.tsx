import type { Span } from '@/api/web';
import { cn } from '@/shell/lib/cn';
import { formatTraceDurationNs } from '@/viz/trace/duration';
import { TraceOperationName } from '@/viz/trace/TraceOperationName';

interface TraceTooltipProps {
  span: Span | null;
  x: number;
  y: number;
  containerRect: DOMRect | null;
}

export function TraceTooltip({ span, x, y, containerRect }: TraceTooltipProps) {
  if (!span || !containerRect) return null;
  // Keep tooltip inside container
  const left = Math.min(x + 12, containerRect.width - 280);
  const top = Math.min(y + 12, containerRect.height - 140);
  const durationNs = span.end_ns - span.start_ns;
  const errorPrefix = span.status === 'ERROR' ? 'ERROR · ' : '';

  return (
    <div
      role="tooltip"
      className={cn(
        'pointer-events-none absolute z-50 w-[280px] rounded-md border border-border bg-surface p-2.5 text-xs shadow-md',
      )}
      style={{ left, top }}
    >
      <div className={cn('mb-1 font-medium', span.status === 'ERROR' && 'text-red')}>
        {errorPrefix}
        {span.service}
        <span className="text-muted-foreground"> · </span>
        <TraceOperationName operation={span.operation} />
      </div>
      <div className="grid grid-cols-2 gap-x-2 gap-y-0.5 text-xs text-muted-foreground">
        <span>duration</span>
        <span className="font-sans text-foreground">{formatTraceDurationNs(durationNs)}</span>
        <span>status</span>
        <span className="font-sans text-foreground">{span.status}</span>
        <span>span_id</span>
        <span className="font-sans text-foreground">{span.span_id.slice(0, 12)}</span>
      </div>
      {Object.keys(span.attributes).length > 0 && (
        <div className="mt-1.5 border-t border-border pt-1 text-xs">
          {Object.entries(span.attributes)
            .slice(0, 5)
            .map(([k, v]) => (
              <div key={k} className="grid grid-cols-2 gap-x-2">
                <span className="truncate text-muted-foreground">{k}</span>
                <span className="truncate font-sans text-foreground">{String(v)}</span>
              </div>
            ))}
        </div>
      )}
    </div>
  );
}

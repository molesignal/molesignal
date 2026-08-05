import { cn } from '@/shell/lib/cn';

import type { LogRow } from './types';

interface HoverPreviewProps {
  row: LogRow | null;
  top: number;
}

export function HoverPreview({ row, top }: HoverPreviewProps) {
  if (!row) return null;
  return (
    <div
      role="tooltip"
      className={cn(
        'pointer-events-none absolute right-3 z-30 max-h-[60vh] w-[420px] overflow-auto rounded-md border border-border bg-surface p-2.5 text-xs shadow-md',
      )}
      style={{ top }}
    >
      <pre className="whitespace-pre-wrap break-words font-sans">{stringifyShort(row)}</pre>
    </div>
  );
}

function stringifyShort(obj: LogRow, limit = 200): string {
  const trimmed: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(obj)) {
    if (typeof v === 'string' && v.length > limit) {
      trimmed[k] = v.slice(0, limit) + '…';
    } else {
      trimmed[k] = v;
    }
  }
  return JSON.stringify(trimmed, null, 2);
}

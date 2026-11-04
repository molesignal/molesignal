import { Construction } from 'lucide-react';

import type { FrameProps } from '@/investigation/frame';
import { JsonView } from '@/shell/JsonEditor';

/**
 * Common stub body shared by frame kinds whose real implementation lands in a
 * later section (8: TimeSeriesChart, 9: TraceFlame, 10: LogStream, 11: topology).
 * Real frame components live next to this file and import it directly.
 */
export function FramePlaceholder({ frame, label }: FrameProps & { label: string }) {
  return (
    <div className="flex h-full flex-col gap-2 p-5">
      <div className="flex items-center gap-2 text-sm text-muted-foreground">
        <Construction className="h-4 w-4" />
        <span>{label} frame – under construction</span>
      </div>
      <JsonView
        value={{ kind: frame.kind, params: frame.params }}
        ariaLabel={`${label} frame`}
        maxHeight={480}
      />
    </div>
  );
}

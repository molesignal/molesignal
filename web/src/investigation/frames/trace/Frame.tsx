import * as React from 'react';

import type { FrameProps } from '@/investigation/frame';
import { useInvestigationStack } from '@/stores/useInvestigationStack';
import { TraceView } from '@/viz/trace/view/TraceView';

import { FramePlaceholder } from '../_placeholder';

export function TraceFrame({ frame }: FrameProps) {
  const push = useInvestigationStack((s) => s.push);
  const traceId = (frame.params.traceId as string | undefined) ?? undefined;
  // The view opens on its root span: only a span the reader picks opens a frame of its own.
  const opened = React.useRef(false);

  if (!traceId) return <FramePlaceholder frame={frame} label="Trace" />;

  return (
    <div className="h-[min(78vh,880px)] min-h-[480px] overflow-hidden rounded-md">
      <TraceView
        traceId={traceId}
        frame="frame"
        onSpanSelect={(span) => {
          if (!opened.current) {
            opened.current = true;
            return;
          }
          if (!span) return;
          push({
            kind: 'trace_span',
            params: { span },
            parentFrameId: frame.id,
          });
        }}
      />
    </div>
  );
}

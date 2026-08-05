import type { FrameProps } from '@/investigation/frame';
import { useInvestigationStack } from '@/stores/useInvestigationStack';
import { TraceFlame } from '@/viz/trace/TraceFlame';

import { FramePlaceholder } from '../_placeholder';

export function TraceFrame({ frame }: FrameProps) {
  const push = useInvestigationStack((s) => s.push);
  const traceId = (frame.params.trace_id as string | undefined) ?? undefined;
  if (!traceId) return <FramePlaceholder frame={frame} label="Trace" />;
  return (
    <TraceFlame
      traceId={traceId}
      onSpanClick={(span) => {
        push({
          kind: 'trace_span',
          params: { span },
          parent_frame_id: frame.id,
        });
      }}
    />
  );
}

import * as React from 'react';

import type { FrameProps } from '@/investigation/frame';
import { useInvestigationStack } from '@/stores/useInvestigationStack';
import { resolveWindow, useTimeStore } from '@/stores/useTimeStore';
import { ServiceTopology } from '@/viz/topology/ServiceTopology';

export function ServiceFrame({ frame }: FrameProps) {
  const window = useTimeStore((s) => s.window);
  const push = useInvestigationStack((s) => s.push);
  const resolved = React.useMemo(() => resolveWindow(window), [window]);
  return (
    <ServiceTopology
      from={resolved.from.toISOString()}
      to={resolved.to.toISOString()}
      onNodeClick={(serviceId) => {
        push({
          kind: 'service',
          params: { service: serviceId },
          parent_frame_id: frame.id,
        });
      }}
      onEdgeClick={(e) => {
        push({
          kind: 'service_to_service',
          params: e,
          parent_frame_id: frame.id,
        });
      }}
    />
  );
}

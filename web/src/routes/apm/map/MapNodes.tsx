import { useTranslation } from 'react-i18next';
import type { NodeProps } from 'reactflow';

import type { ServiceHealth } from '@/api/apm';
import type { MapChange, MapNode } from '@/api/apmMap';
import type { HealthLines } from '@/api/apmSettings';
import { cn } from '@/shell/lib/cn';
import { type MapNodeShape, MapNodeView } from '@/viz/topology/MapNodeView';

export interface MapNodeData {
  name: string;
  /** What the node is: a service, something called, a topic, or a folded namespace. */
  shape: MapNodeShape;
  health?: ServiceHealth | undefined;
  /** What it served or was asked, as one line. */
  figures: string;
  change?: MapChange | undefined;
  /** Out of the reach of the node chosen. */
  dimmed: boolean;
  chosen: boolean;
}

/** How a node is drawn, from what it is. */
export function nodeShape(node: MapNode): MapNodeShape {
  if (node.kind === 'topic') return 'topic';
  if (node.kind === 'service') return node.health ? 'service' : 'quiet_service';

  switch (node.category) {
    case 'database':
    case 'cache':
    case 'external_http':
    case 'external_rpc':
      return node.category;
    case 'service':
      return 'quiet_service';
    default:
      return 'other';
  }
}

/**
 * The health a node's ring says: a service's own, else read from how its
 * calls fail against the organization's error rate lines.
 */
export function nodeHealth(
  node: { health?: ServiceHealth | undefined; red: { requestCount: number; errorRate: number } },
  lines: Pick<HealthLines, 'errorRateWarning' | 'errorRateCritical'>,
): ServiceHealth {
  if (node.health) return node.health;
  if (node.red.requestCount === 0) return 'no_traffic';
  if (node.red.errorRate >= lines.errorRateCritical) return 'critical';
  if (node.red.errorRate >= lines.errorRateWarning) return 'warning';
  return 'healthy';
}

/**
 * A node of the APM map, drawn as every service map draws its nodes; a node
 * new against the stretch before, or gone since, says so after its name.
 */
export function ApmMapNode({ data }: NodeProps<MapNodeData>) {
  const { t } = useTranslation('apm');
  const health = data.health ?? 'no_traffic';

  return (
    <div data-apm-map-node={data.name} data-shape={data.shape} data-health={health} data-dimmed={data.dimmed ? 'true' : 'false'}>
      <MapNodeView
        name={data.name}
        shape={data.shape}
        tone={health}
        figures={data.figures}
        dimmed={data.dimmed}
        gone={data.change === 'gone'}
        chosen={data.chosen}
        after={data.change && <span className={cn('shrink-0 font-strong', data.change === 'new' ? 'text-[var(--green-ink)]' : 'text-tx-3')}>{t(`map.change.${data.change}`)}</span>}
      />
    </div>
  );
}

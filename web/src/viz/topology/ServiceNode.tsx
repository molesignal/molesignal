import {
  CircleDollarSign,
  Database,
  Flag,
  Globe2,
  Image,
  Mail,
  Megaphone,
  MessageSquare,
  Network,
  Server,
  ShoppingCart,
  type LucideIcon,
} from 'lucide-react';
import type { NodeProps } from 'reactflow';

import { SignalReference } from '@/shell/SignalReference';
import { useTopologyFlags } from '@/stores/useTopologyFlags';

import type { TopologyDirection } from './forceLayout';
import { MapNodeView } from './MapNodeView';

export interface ServiceNodeData {
  name: string;
  error_rate: number;
  p95_ms: number;
  rps: number;
  span_count: number;
  matchesSearch?: boolean;
  showMetrics?: boolean;
  showType?: boolean;
  direction?: TopologyDirection;
}

export function ServiceNode({ id, data }: NodeProps<ServiceNodeData>) {
  // Warning status honors the hysteresis stored in useTopologyFlags;
  // ServiceTopology calls `applyHysteresis(nodes)` after each load so the
  // store contains a per-id boolean. Fallback to the raw threshold so
  // first render (before the store seeds) still behaves sensibly.
  const flagged = useTopologyFlags((s) => s.redRing[id]);
  const health = serviceHealthStatus(data.error_rate, flagged ?? data.error_rate >= 0.05);
  const matchesSearch = data.matchesSearch !== false;

  return (
    <div data-health={health} data-search-match={matchesSearch ? 'true' : 'false'} data-testid={`topology-node-${id}`} aria-label={`${data.name}, ${health}`}>
      <MapNodeView
        name={data.name}
        shape="service"
        icon={data.showType === false ? undefined : iconForService(data.name)}
        tone={health}
        dimmed={!matchesSearch}
        vertical={data.direction === 'vertical'}
        draggable
        figures={data.showMetrics === false ? undefined : figures(data)}
        label={
          <SignalReference
            type="service"
            value={data.name}
            className="nodrag nopan block truncate !font-strong !text-tx-0 !no-underline hover:!text-tx-0 hover:!no-underline [&>svg]:hidden"
          >
            {truncate(data.name, 26)}
          </SignalReference>
        }
      />
    </div>
  );
}

/** What a service served, as every service map says it: requests a second, failures, P95. */
function figures(data: ServiceNodeData): string {
  const perSecond = data.rps >= 100 ? data.rps.toFixed(0) : data.rps >= 10 ? data.rps.toFixed(1) : data.rps.toFixed(2);
  const p95 = data.p95_ms >= 1_000 ? `${(data.p95_ms / 1_000).toFixed(2)} s` : `${data.p95_ms.toFixed(data.p95_ms < 10 ? 1 : 0)} ms`;
  return `${perSecond} req/s · ${(data.error_rate * 100).toFixed(2)}% · P95 ${p95}`;
}

export type ServiceHealthStatus = 'healthy' | 'degraded' | 'warning' | 'critical';

export function serviceHealthStatus(errorRate: number, warningLatched = false): ServiceHealthStatus {
  if (errorRate >= 0.1) return 'critical';
  if (warningLatched || errorRate >= 0.05) return 'warning';
  if (errorRate >= 0.01) return 'degraded';
  return 'healthy';
}

function iconForService(name: string): LucideIcon {
  const normalized = name.toLowerCase();
  if (/(postgres|mysql|mongo|redis|database|\bdb\b|storage)/.test(normalized)) return Database;
  if (/(cart|checkout|order)/.test(normalized)) return ShoppingCart;
  if (/(currency|account|billing|payment)/.test(normalized)) return CircleDollarSign;
  if (/(kafka|queue|rabbit|event|message)/.test(normalized)) return MessageSquare;
  if (/(mail|email)/.test(normalized)) return Mail;
  if (/(image|media|asset)/.test(normalized)) return Image;
  if (/(flag|feature)/.test(normalized)) return Flag;
  if (/(advert|\bad\b)/.test(normalized)) return Megaphone;
  if (/(frontend|web|browser)/.test(normalized)) return Globe2;
  if (/(gateway|proxy|grpc|http|api)/.test(normalized)) return Network;
  return Server;
}

function truncate(name: string, max: number): string {
  return name.length <= max ? name : `${name.slice(0, max - 1)}…`;
}

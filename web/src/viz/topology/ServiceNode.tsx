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
import { Handle, Position, type NodeProps } from 'reactflow';

import { cn } from '@/shell/lib/cn';
import { SignalReference } from '@/shell/SignalReference';
import { useTopologyFlags } from '@/stores/useTopologyFlags';
import { useThemePalette } from '@/viz/timeseries/themeAdapter';

import type { TopologyDirection } from './forceLayout';

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
  const { palette } = useThemePalette();
  // Warning status honors the hysteresis stored in useTopologyFlags;
  // ServiceTopology calls `applyHysteresis(nodes)` after each load so the
  // store contains a per-id boolean. Fallback to the raw threshold so
  // first render (before the store seeds) still behaves sensibly.
  const flagged = useTopologyFlags((s) => s.redRing[id]);
  const health = serviceHealthStatus(data.error_rate, flagged ?? data.error_rate >= 0.05);
  const healthColor = {
    healthy: palette['--green'],
    degraded: palette['--yellow'],
    warning: 'var(--orange)',
    critical: palette['--red'],
  }[health];
  const ServiceIcon = iconForService(data.name);
  const matchesSearch = data.matchesSearch !== false;
  const targetPosition = data.direction === 'vertical' ? Position.Top : Position.Left;
  const sourcePosition = data.direction === 'vertical' ? Position.Bottom : Position.Right;

  return (
    <div
      className={cn(
        'relative flex min-w-[184px] items-center gap-2.5 rounded-md px-1 py-0.5 font-sans text-xs transition-opacity duration-fast',
        matchesSearch ? 'opacity-100' : 'opacity-20',
      )}
      data-health={health}
      data-search-match={matchesSearch ? 'true' : 'false'}
      data-testid={`topology-node-${id}`}
    >
      <Handle type="target" position={targetPosition} className="!h-1 !w-1 !border-0 !bg-transparent" />
      <div
        data-topology-drag-handle
        className="grid h-8 w-8 shrink-0 cursor-grab place-items-center rounded-full border-2 bg-bg-1 text-tx-1 active:cursor-grabbing"
        style={{ borderColor: healthColor }}
        aria-label={`${data.name}, ${health}`}
      >
        {data.showType === false ? (
          <span className="h-1.5 w-1.5 rounded-full" style={{ backgroundColor: healthColor }} />
        ) : (
          <ServiceIcon className="h-3.5 w-3.5" aria-hidden="true" />
        )}
      </div>
      <div className="min-w-0 max-w-[188px] leading-tight">
        <SignalReference
          type="service"
          value={data.name}
          className="nodrag nopan block truncate !font-semibold !text-tx-0 !no-underline hover:!text-tx-0 hover:!no-underline [&>svg]:hidden"
        >
          {truncate(data.name, 26)}
        </SignalReference>
        {data.showMetrics !== false && (
          <span className="type-micro mt-0.5 block truncate font-medium text-tx-2">
            {formatCompact(data.span_count)} spans · {formatCompact(data.rps)} rps
          </span>
        )}
      </div>
      <Handle type="source" position={sourcePosition} className="!h-1 !w-1 !border-0 !bg-transparent" />
    </div>
  );
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

function formatCompact(value: number): string {
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(value >= 10_000_000 ? 0 : 1)}m`;
  if (value >= 1000) return `${(value / 1000).toFixed(value >= 10_000 ? 0 : 1)}k`;
  if (value >= 10) return value.toFixed(0);
  return value.toFixed(value > 0 && value < 1 ? 1 : 0);
}

function truncate(name: string, max: number): string {
  return name.length <= max ? name : `${name.slice(0, max - 1)}…`;
}

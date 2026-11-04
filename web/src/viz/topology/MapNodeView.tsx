import { Boxes, Database, Globe2, MessagesSquare, Network, Server, ServerOff, Zap, type LucideIcon } from 'lucide-react';
import type * as React from 'react';
import { Handle, Position } from 'reactflow';

import { cn } from '@/shell/lib/cn';

/** What a node of a service map is: a service, something called, a topic, or a folded namespace. */
export type MapNodeShape = 'service' | 'quiet_service' | 'database' | 'cache' | 'external_http' | 'external_rpc' | 'other' | 'topic' | 'group';

/** How a node fares, as the colour of its ring says. */
export type MapNodeTone = 'healthy' | 'degraded' | 'warning' | 'critical' | 'no_traffic';

const ICONS: Record<MapNodeShape, LucideIcon> = {
  service: Server,
  quiet_service: ServerOff,
  database: Database,
  cache: Zap,
  external_http: Globe2,
  external_rpc: Network,
  other: Server,
  topic: MessagesSquare,
  group: Boxes,
};

const RING: Record<MapNodeTone, string> = {
  healthy: 'var(--green)',
  degraded: 'var(--yellow)',
  warning: 'var(--orange)',
  critical: 'var(--red)',
  no_traffic: 'var(--tx-4)',
};

/**
 * A node of a service map, as every map draws it: a ring in the colour of
 * how the node fares around what it is, its name, and a line of what it
 * served. Something called and a topic are square; a service and a folded
 * namespace are round. A node out of the reach of the one chosen fades.
 */
export function MapNodeView({
  name,
  label,
  shape,
  icon,
  tone,
  figures,
  after,
  dimmed = false,
  gone = false,
  chosen = false,
  vertical = false,
  draggable = false,
}: {
  name: string;
  /** The name as drawn, when it is more than text (a reference menu). */
  label?: React.ReactNode;
  shape: MapNodeShape;
  /** An icon in place of the shape's own. */
  icon?: LucideIcon | undefined;
  tone: MapNodeTone;
  figures?: string | undefined;
  /** Words after the name: a change against the stretch before. */
  after?: React.ReactNode;
  dimmed?: boolean;
  gone?: boolean;
  chosen?: boolean;
  /** Calls run top to bottom rather than left to right. */
  vertical?: boolean;
  /** The node is moved by its ring. */
  draggable?: boolean;
}) {
  const Icon = icon ?? ICONS[shape];
  const square = shape !== 'service' && shape !== 'quiet_service' && shape !== 'group';

  return (
    <div className={cn('relative flex min-w-[168px] items-center gap-2.5 px-1 py-0.5 font-sans text-xs transition-opacity duration-fast', dimmed ? 'opacity-25' : gone ? 'opacity-50' : 'opacity-100')}>
      <Handle type="target" position={vertical ? Position.Top : Position.Left} className="!h-1 !w-1 !border-0 !bg-transparent" />
      <span
        data-topology-drag-handle={draggable ? '' : undefined}
        className={cn('grid h-8 w-8 shrink-0 place-items-center bg-bg-1 text-tx-1', square ? 'rounded-md' : 'rounded-full', chosen ? 'border-[3px]' : 'border-2', draggable && 'cursor-grab active:cursor-grabbing')}
        style={{ borderColor: RING[tone] }}
      >
        <Icon aria-hidden className="h-3.5 w-3.5" />
      </span>
      <span className="min-w-0 max-w-[200px] leading-tight">
        <span className="flex min-w-0 items-center gap-1.5">
          {label ?? <span className={cn('truncate font-strong text-tx-0', gone && 'line-through')}>{name}</span>}
          {after}
        </span>
        {figures && <span className="type-micro mt-0.5 block truncate text-tx-2">{figures}</span>}
      </span>
      <Handle type="source" position={vertical ? Position.Bottom : Position.Right} className="!h-1 !w-1 !border-0 !bg-transparent" />
    </div>
  );
}

/** The colour of a call between two nodes: red from 5% failed, orange from 1%. */
export function callStroke(errorRate: number, palette: Readonly<Record<string, string>>): string {
  if (errorRate >= 0.05) return palette['--red'] ?? 'var(--red)';
  if (errorRate >= 0.01) return 'var(--orange)';
  return palette['--border'] ?? 'var(--border)';
}

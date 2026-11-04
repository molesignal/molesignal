import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { cn } from '@/shell/lib/cn';

import { formatTraceDurationNs } from '../duration';
import { traceServiceMap, type TraceMapCall, type TraceMapService } from '../model/serviceMap';
import type { TraceTree } from '../model/tree';
import { useMeasuredWidth } from '../waterfall/Axis';

const NODE_WIDTH = 200;
const NODE_HEIGHT = 52;
/** The room one call into a service takes on its side: the line arrives there and its label sits on it. */
const PORT_ROW = 18;
/** A call some of whose spans failed says so on a second line. */
const FAILED_PORT_ROW = 34;
/** Between two columns: room for the labels of the calls, never less, and never so much the map falls apart. */
const LEAST_GAP = 136;
const MOST_GAP = 220;
const ROW_GAP = 20;
const PAD = 16;

interface Placed {
  service: TraceMapService;
  x: number;
  y: number;
  height: number;
}

interface Line {
  call: TraceMapCall;
  startX: number;
  startY: number;
  endX: number;
  endY: number;
}

/**
 * The services of this one trace and the calls between them, left to right
 * from where the trace enters: which service called which, how often, how
 * long the calls took and whether they failed. Each call arrives at its own
 * place on the side of the service it called, with its label on it, so no
 * two labels meet. A service opens its spans in the waterfall.
 */
export function TraceMap({ tree, onOpenService }: { tree: TraceTree; onOpenService: (service: string) => void }) {
  const { t } = useTranslation('traces');
  const [frameRef, available] = useMeasuredWidth<HTMLDivElement>();
  const map = React.useMemo(() => traceServiceMap(tree), [tree]);
  const { nodes, lines, width, height, gap } = React.useMemo(() => layout(map.services, map.calls, available), [available, map]);

  return (
    <div ref={frameRef} data-testid="trace-map" className="h-full min-h-0 overflow-auto">
      <div className="relative mx-auto my-4" style={{ width, height }}>
        <svg aria-hidden="true" width={width} height={height} className="absolute inset-0 overflow-visible">
          {lines.map((line) => (
            <CallLine key={callKey(line.call)} line={line} />
          ))}
        </svg>
        {lines.map(({ call, endX, endY }) => {
          const text = `${t('view.map.calls', { count: call.calls })} · ${formatTraceDurationNs(call.durationNs / call.calls)}`;
          const failed = call.errors > 0 ? t('view.map.errors', { count: call.errors }) : '';

          return (
            <span
              key={`label-${callKey(call)}`}
              data-testid="trace-map-call"
              title={failed ? `${text} · ${failed}` : text}
              className="absolute flex -translate-y-1/2 flex-col items-end rounded-sm bg-[var(--functional-surface)] px-1 font-sans text-xs tabular-nums leading-4 text-tx-2"
              style={{ right: width - endX + 6, top: endY, maxWidth: gap - 20 }}
            >
              <span className="max-w-full truncate whitespace-nowrap">{text}</span>
              {failed && <span className="max-w-full truncate whitespace-nowrap text-[var(--red-ink)]">{failed}</span>}
            </span>
          );
        })}
        {nodes.map(({ service, x, y, height: nodeHeight }) => (
          <button
            key={service.name}
            type="button"
            data-testid="trace-map-service"
            onClick={() => onOpenService(service.name)}
            className={cn(
              'absolute flex flex-col justify-center gap-1 rounded-md bg-bg-2 px-3 text-left',
              'hover:bg-bg-3 focus-visible:bg-bg-3 focus-visible:outline-none',
            )}
            style={{ left: x, top: y, width: NODE_WIDTH, height: nodeHeight }}
          >
            <span className="flex w-full min-w-0 items-center gap-1.5">
              <span aria-hidden="true" className="h-2 w-2 shrink-0 rounded-sm" style={{ backgroundColor: service.color }} />
              <span className="min-w-0 truncate font-sans text-xs font-strong text-tx-0">{service.name}</span>
              {service.entry && (
                <span className="type-micro shrink-0 rounded-sm bg-[var(--indigo-tint)] px-1 font-strong text-indigo">{t('view.map.entry')}</span>
              )}
              {service.errors > 0 && (
                <span className="ml-auto shrink-0 whitespace-nowrap font-sans text-xs text-[var(--red-ink)]">{t('view.map.errors', { count: service.errors })}</span>
              )}
            </span>
            <span className="w-full truncate whitespace-nowrap font-sans text-xs tabular-nums text-tx-2">
              {t('view.map.spans', { count: service.spans })}
              <span className="mx-1 text-tx-4">·</span>
              {t('view.map.share', { percent: Math.round(service.share * 1000) / 10 })}
            </span>
          </button>
        ))}
      </div>
    </div>
  );
}

function CallLine({ line }: { line: Line }) {
  const { call, startX, startY, endX, endY } = line;
  const bend = Math.max(40, Math.abs(endX - startX) / 2);

  return (
    <path
      d={`M ${startX} ${startY} C ${startX + bend} ${startY}, ${endX - bend} ${endY}, ${endX} ${endY}`}
      fill="none"
      stroke={call.errors > 0 ? 'var(--red)' : 'var(--bd-2)'}
      strokeWidth={Math.min(4, 1 + Math.log2(call.calls))}
    />
  );
}

function callKey(call: TraceMapCall): string {
  return `${call.from}\u0000${call.to}`;
}

function portRow(call: TraceMapCall): number {
  return call.errors > 0 ? FAILED_PORT_ROW : PORT_ROW;
}

/** Columns by how many calls away from the entry a service is first reached; services no call reaches go last. */
function levels(services: readonly TraceMapService[], calls: readonly TraceMapCall[]): Map<string, number> {
  const level = new Map<string, number>();
  const entry = services.find((service) => service.entry) ?? services[0];
  if (entry) level.set(entry.name, 0);

  const queue = entry ? [entry.name] : [];
  while (queue.length > 0) {
    const name = queue.shift()!;
    for (const call of calls) {
      if (call.from !== name || level.has(call.to)) continue;
      level.set(call.to, (level.get(name) ?? 0) + 1);
      queue.push(call.to);
    }
  }

  const deepest = Math.max(0, ...level.values());
  for (const service of services) {
    if (!level.has(service.name)) level.set(service.name, deepest + 1);
  }

  return level;
}

/**
 * Where everything goes. A column's services stand in the order of the
 * services calling them, so lines cross as little as they can; a service is
 * as tall as the calls arriving at it need; the columns spread over the room
 * there is, within limits.
 */
function layout(services: readonly TraceMapService[], calls: readonly TraceMapCall[], available: number) {
  const level = levels(services, calls);
  const columnCount = Math.max(0, ...level.values()) + 1;
  const spread = columnCount > 1 ? (available - 2 * PAD - columnCount * NODE_WIDTH) / (columnCount - 1) : LEAST_GAP;
  const gap = Math.min(MOST_GAP, Math.max(LEAST_GAP, spread));

  const incoming = new Map<string, TraceMapCall[]>();
  for (const call of calls) incoming.set(call.to, [...(incoming.get(call.to) ?? []), call]);

  const placed = new Map<string, Placed>();
  const centre = (node: Placed) => node.y + node.height / 2;
  const callersCentre = (name: string) => {
    const callers = (incoming.get(name) ?? []).map((call) => placed.get(call.from)).filter((node): node is Placed => node !== undefined);
    return callers.length > 0 ? callers.reduce((sum, node) => sum + centre(node), 0) / callers.length : Number.MAX_SAFE_INTEGER;
  };

  for (let column = 0; column < columnCount; column += 1) {
    const members = services
      .filter((service) => level.get(service.name) === column)
      .map((service, index) => ({ service, index, key: callersCentre(service.name) }))
      .sort((a, b) => a.key - b.key || a.index - b.index);

    let y = PAD;
    for (const { service } of members) {
      const arriving = (incoming.get(service.name) ?? []).reduce((sum, call) => sum + portRow(call), 0);
      const height = Math.max(NODE_HEIGHT, arriving + 10);
      placed.set(service.name, { service, x: PAD + column * (NODE_WIDTH + gap), y, height });
      y += height + ROW_GAP;
    }
  }

  // Calls leave a service spread down its side in the order of where they go, and arrive the same way.
  const leaving = new Map<string, number>();
  for (const node of placed.values()) {
    const out = calls
      .filter((call) => call.from === node.service.name && placed.has(call.to))
      .sort((a, b) => centre(placed.get(a.to)!) - centre(placed.get(b.to)!));
    const step = out.length > 1 ? Math.min(10, (node.height - 16) / (out.length - 1)) : 0;
    out.forEach((call, index) => leaving.set(callKey(call), centre(node) + (index - (out.length - 1) / 2) * step));
  }

  const lines: Line[] = [];
  for (const node of placed.values()) {
    const into = (incoming.get(node.service.name) ?? [])
      .filter((call) => placed.has(call.from))
      .sort((a, b) => centre(placed.get(a.from)!) - centre(placed.get(b.from)!));

    let arrival = centre(node) - into.reduce((sum, call) => sum + portRow(call), 0) / 2;
    for (const call of into) {
      const from = placed.get(call.from)!;
      lines.push({
        call,
        startX: from.x + NODE_WIDTH,
        startY: leaving.get(callKey(call)) ?? centre(from),
        endX: node.x,
        endY: arrival + portRow(call) / 2,
      });
      arrival += portRow(call);
    }
  }

  const nodes = [...placed.values()];
  const width = Math.max(NODE_WIDTH, ...nodes.map((node) => node.x + NODE_WIDTH)) + PAD;
  const height = Math.max(NODE_HEIGHT, ...nodes.map((node) => node.y + node.height)) + PAD;

  return { nodes, lines, width, height, gap };
}

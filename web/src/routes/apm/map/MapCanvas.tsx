import * as React from 'react';
import { Background, BackgroundVariant, Controls, MarkerType, ReactFlow, type Edge, type Node, type ReactFlowInstance } from 'reactflow';
import 'reactflow/dist/style.css';

import type { TopologyEdge, TopologyNode } from '@/api/web';
import { isNestedControlClick } from '@/shell/lib/nestedControl';
import { useThemePalette } from '@/viz/timeseries/themeAdapter';
import { computeLayeredLayout } from '@/viz/topology/forceLayout';

import type { DrawnEdge } from './graph';
import { ApmMapNode, type MapNodeData } from './MapNodes';

const NODE_TYPES = { apm: ApmMapNode };
const FIT_VIEW_OPTIONS = { padding: 0.16, maxZoom: 1.1 };

/** A node as the canvas lays it out and draws it. */
export interface CanvasNode {
  id: string;
  data: MapNodeData;
}

export interface CanvasEdge extends DrawnEdge {
  dimmed: boolean;
  /** On the slowest way down from the node chosen. */
  critical: boolean;
}

/**
 * The map drawn: its nodes laid out in layers from callers to callees, each
 * call an arrow as thick as its traffic and in the colour of how it fails;
 * a call gone since the stretch before is dashed. A node or a call is
 * chosen by a click.
 */
export function MapCanvas({
  nodes,
  edges,
  onNode,
  onEdge,
  onPane,
}: {
  nodes: readonly CanvasNode[];
  edges: readonly CanvasEdge[];
  onNode: (id: string) => void;
  onEdge: (edge: CanvasEdge) => void;
  onPane: () => void;
}) {
  const { palette } = useThemePalette();
  const [container, setContainer] = React.useState<HTMLDivElement | null>(null);
  const [size, setSize] = React.useState<{ width: number; height: number } | null>(null);
  const [instance, setInstance] = React.useState<ReactFlowInstance | null>(null);
  const fitted = React.useRef('');

  React.useLayoutEffect(() => {
    if (!container) return;

    const measure = () => {
      const width = container.clientWidth;
      const height = container.clientHeight;
      if (width > 0 && height > 0) setSize((current) => (current && current.width === width && current.height === height ? current : { width, height }));
    };

    measure();
    if (typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(measure);
    observer.observe(container);
    return () => observer.disconnect();
  }, [container]);

  // The layout follows the nodes and calls, not what is chosen among them.
  const shapeKey = `${nodes.map((node) => node.id).join(',')}|${edges.map((edge) => edge.id).join(',')}|${size?.width ?? 0}x${size?.height ?? 0}`;
  const positions = React.useMemo(() => {
    if (!size) return {};
    const layoutNodes = nodes.map((node) => ({ id: node.id, name: node.data.name, errorRate: 0, p95Ms: 0, rps: 0, spanCount: 0 }) satisfies TopologyNode);
    const layoutEdges = edges.map((edge) => ({ source: edge.source, target: edge.target, rps: 0, errRate: 0, p95Ms: 0 }) satisfies TopologyEdge);
    return computeLayeredLayout(layoutNodes, layoutEdges, size.width, size.height, 'horizontal');
  }, [edges, nodes, size]);

  const busiest = Math.max(1, ...edges.map((edge) => edge.red.requestCount));
  const flowNodes = nodes.map<Node<MapNodeData>>((node) => ({
    id: node.id,
    type: 'apm',
    position: positions[node.id] ?? { x: 0, y: 0 },
    data: node.data,
  }));
  const flowEdges = edges.map<Edge>((edge) => {
    const stroke = edge.critical
      ? palette['--accent']
      : edge.change === 'new'
        ? 'var(--green)'
        : edge.red.errorRate >= 0.05
          ? palette['--red']
          : edge.red.errorRate >= 0.01
            ? 'var(--orange)'
            : palette['--border'];
    return {
      id: edge.id,
      source: edge.source,
      target: edge.target,
      markerEnd: { type: MarkerType.ArrowClosed, width: 14, height: 14, color: stroke },
      style: {
        stroke,
        strokeWidth: (edge.critical ? 2.4 : 1.2) + Math.min(1.8, (edge.red.requestCount / busiest) * 1.8),
        strokeDasharray: edge.change === 'gone' ? '5 4' : undefined,
        opacity: edge.dimmed ? 0.12 : edge.change === 'gone' ? 0.5 : 0.85,
      },
      data: edge,
    };
  });

  // React Flow fits nothing before it has measured the nodes: each new
  // shape is fitted on the first frame its nodes carry their sizes.
  React.useEffect(() => {
    if (!instance || !size || fitted.current === shapeKey) return;
    let frame = 0;
    let tries = 0;
    const fit = () => {
      const drawn = instance.getNodes();
      if (drawn.length > 0 && drawn.every((node) => node.width && node.height) && instance.fitView(FIT_VIEW_OPTIONS)) {
        fitted.current = shapeKey;
        return;
      }
      if ((tries += 1) < 30) frame = requestAnimationFrame(fit);
    };
    frame = requestAnimationFrame(fit);
    return () => cancelAnimationFrame(frame);
  }, [instance, shapeKey, size]);

  return (
    <div ref={setContainer} data-apm-map-canvas="" className="h-full w-full">
      {size && (
        <ReactFlow
          nodes={flowNodes}
          edges={flowEdges}
          nodeTypes={NODE_TYPES}
          onInit={setInstance}
          fitView
          fitViewOptions={FIT_VIEW_OPTIONS}
          minZoom={0.1}
          maxZoom={1.75}
          nodesDraggable={false}
          nodesConnectable={false}
          elementsSelectable={false}
          onNodeClick={(event, node) => {
            if (!isNestedControlClick(event)) onNode(node.id);
          }}
          onEdgeClick={(_, edge) => onEdge(edge.data as CanvasEdge)}
          onPaneClick={onPane}
          proOptions={{ hideAttribution: true }}
        >
          <Background variant={BackgroundVariant.Dots} gap={18} color={palette['--surface-muted']} />
          <Controls
            position="bottom-right"
            className="!border !border-bd-1 !bg-bg-1 !shadow-sm [&>button]:!border-bd-0 [&>button]:!bg-bg-1 [&>button]:!fill-current [&>button]:!text-tx-1 [&>button:hover]:!bg-bg-3"
            showInteractive={false}
          />
        </ReactFlow>
      )}
    </div>
  );
}

import * as React from 'react';
import {
  Background,
  BackgroundVariant,
  Controls,
  MiniMap,
  ReactFlow,
  useEdgesState,
  useNodesState,
  type Edge,
  type NodeMouseHandler,
  type Node,
  type ReactFlowInstance,
} from 'reactflow';
import 'reactflow/dist/style.css';

import type { TopologyResponse } from '@/api/web';
import { useTopologyFlags } from '@/stores/useTopologyFlags';
import { useThemePalette } from '@/viz/timeseries/themeAdapter';

import {
  computeForceLayout,
  computeLayeredLayout,
  hashGraph,
  type TopologyDirection,
  useTopologyLayoutCache,
} from './forceLayout';
import { useTopology } from './loader';
import { ServiceEdge, type ServiceEdgeData } from './ServiceEdge';
import { ServiceNode, type ServiceNodeData } from './ServiceNode';

export type TopologyLayoutMode = 'tree' | 'graph';

interface ServiceTopologyProps {
  from: string;
  to: string;
  topology?: TopologyResponse | undefined;
  layout?: TopologyLayoutMode;
  direction?: TopologyDirection;
  searchQuery?: string;
  showNodeMetrics?: boolean;
  showServiceTypes?: boolean;
  showEdgeMetrics?: boolean;
  showBackground?: boolean;
  showMiniMap?: boolean;
  onNodeClick?: (serviceId: string) => void;
  onEdgeClick?: (e: { source: string; target: string }) => void;
}

const NODE_TYPES = { service: ServiceNode };
const EDGE_TYPES = { service: ServiceEdge };

export function ServiceTopology({
  from,
  to,
  topology,
  layout = 'graph',
  direction = 'horizontal',
  searchQuery = '',
  showNodeMetrics = true,
  showServiceTypes = true,
  showEdgeMetrics = true,
  showBackground = true,
  showMiniMap = true,
  onNodeClick,
  onEdgeClick,
}: ServiceTopologyProps) {
  const containerRef = React.useRef<HTMLDivElement | null>(null);
  const instanceRef = React.useRef<ReactFlowInstance | null>(null);
  const [viewportSize, setViewportSize] = React.useState({ width: 800, height: 600 });
  const topologyQuery = useTopology(from, to);
  const data = topology ?? topologyQuery.data;
  const isLoading = topology === undefined && topologyQuery.isLoading;
  const { palette } = useThemePalette();
  const getCachedLayout = useTopologyLayoutCache((state) => state.get);
  const setCachedLayout = useTopologyLayoutCache((state) => state.set);
  const applyHysteresis = useTopologyFlags((s) => s.applyHysteresis);

  React.useEffect(() => {
    const element = containerRef.current;
    if (!element) return;

    const updateSize = () => {
      const width = element.clientWidth;
      const height = element.clientHeight;
      if (width > 0 && height > 0) {
        setViewportSize((current) =>
          current.width === width && current.height === height
            ? current
            : { width, height },
        );
      }
    };

    updateSize();
    if (typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(updateSize);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  React.useEffect(() => {
    if (data) applyHysteresis(data.nodes);
  }, [data, applyHysteresis]);

  const layoutKey = React.useMemo(() => {
    if (!data) return '';
    return `${layout}:${direction}:${hashGraph(
      data.nodes,
      data.edges,
      viewportSize.width,
      viewportSize.height,
    )}`;
  }, [data, direction, layout, viewportSize]);

  const positions = React.useMemo(() => {
    if (!data) return {} as Record<string, { x: number; y: number }>;
    const cached = getCachedLayout(layoutKey);
    if (cached) return cached;
    return layout === 'tree'
      ? computeLayeredLayout(
          data.nodes,
          data.edges,
          viewportSize.width,
          viewportSize.height,
          direction,
        )
      : computeForceLayout(
          data.nodes,
          data.edges,
          viewportSize.width,
          viewportSize.height,
        );
  }, [data, direction, getCachedLayout, layout, layoutKey, viewportSize]);

  React.useEffect(() => {
    if (!layoutKey || Object.keys(positions).length === 0) return;
    if (!getCachedLayout(layoutKey)) setCachedLayout(layoutKey, positions);
  }, [getCachedLayout, layoutKey, positions, setCachedLayout]);

  const built = React.useMemo<{ nodes: Node<ServiceNodeData>[]; edges: Edge<ServiceEdgeData>[] }>(() => {
    if (!data) return { nodes: [], edges: [] };
    const normalizedSearch = searchQuery.trim().toLocaleLowerCase();
    const matchedIds = new Set(
      data.nodes
        .filter((node) => !normalizedSearch || node.name.toLocaleLowerCase().includes(normalizedSearch))
        .map((node) => node.id),
    );
    const maxRps = Math.max(1, ...data.edges.map((edge) => edge.rps));
    return {
      nodes: data.nodes.map((n) => ({
        id: n.id,
        type: 'service',
        position: positions[n.id] ?? { x: 0, y: 0 },
        data: {
          name: n.name,
          error_rate: n.error_rate,
          p95_ms: n.p95_ms,
          rps: n.rps,
          span_count: n.span_count,
          matchesSearch: matchedIds.has(n.id),
          showMetrics: showNodeMetrics,
          showType: showServiceTypes,
          direction,
        },
      })),
      edges: data.edges.map((e) => {
        const touchesMatch = matchedIds.has(e.source) || matchedIds.has(e.target);
        return {
          id: `${e.source}->${e.target}`,
          source: e.source,
          target: e.target,
          type: 'service',
          data: {
            rps: e.rps,
            err_rate: e.err_rate,
            p95_ms: e.p95_ms,
            showLabel: showEdgeMetrics,
          },
          style: {
            stroke: palette['--border'],
            strokeWidth: 1.2 + Math.min(1.8, (e.rps / maxRps) * 1.8),
            opacity: normalizedSearch ? (touchesMatch ? 0.9 : 0.12) : 0.72,
          },
          animated: false,
        };
      }),
    };
  }, [data, direction, palette, positions, searchQuery, showEdgeMetrics, showNodeMetrics, showServiceTypes]);

  const [nodes, setNodes, onNodesChange] = useNodesState(built.nodes);
  const [edges, setEdges, onEdgesChange] = useEdgesState(built.edges);
  const lastLayoutKeyRef = React.useRef('');
  const nodesRef = React.useRef(nodes);
  nodesRef.current = nodes;

  React.useEffect(() => {
    const layoutChanged = lastLayoutKeyRef.current !== layoutKey;
    setNodes((current) => {
      const currentPositions = new Map(current.map((node) => [node.id, node.position]));
      return built.nodes.map((node) => ({
        ...node,
        position: layoutChanged
          ? node.position
          : (currentPositions.get(node.id) ?? node.position),
      }));
    });
    setEdges(built.edges);
    lastLayoutKeyRef.current = layoutKey;
  }, [built, layoutKey, setEdges, setNodes]);

  React.useEffect(() => {
    if (!layoutKey || !instanceRef.current) return;
    const frame = globalThis.requestAnimationFrame(() => {
      instanceRef.current?.fitView({ padding: 0.18, maxZoom: 1.1, duration: 180 });
    });
    return () => globalThis.cancelAnimationFrame(frame);
  }, [layoutKey]);

  const handleNodeDragStop = React.useCallback<NodeMouseHandler>((_, draggedNode) => {
    if (!layoutKey) return;
    const nextPositions = Object.fromEntries(
      nodesRef.current.map((node) => [
        node.id,
        node.id === draggedNode.id ? draggedNode.position : node.position,
      ]),
    );
    setCachedLayout(layoutKey, nextPositions);
  }, [layoutKey, setCachedLayout]);

  if (isLoading) return <div className="p-5 text-sm text-muted-foreground">Loading topology…</div>;
  if (!data || data.nodes.length === 0) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
        No service traffic in window
      </div>
    );
  }

  return (
    <div ref={containerRef} className="h-full w-full">
      <ReactFlow
        nodes={nodes}
        edges={edges}
        onNodesChange={onNodesChange}
        onEdgesChange={onEdgesChange}
        onNodeDragStop={handleNodeDragStop}
        nodeTypes={NODE_TYPES}
        edgeTypes={EDGE_TYPES}
        onInit={(instance) => {
          instanceRef.current = instance;
        }}
        fitView
        fitViewOptions={{ padding: 0.18, maxZoom: 1.1 }}
        minZoom={0.15}
        maxZoom={1.75}
        nodesDraggable
        nodesConnectable={false}
        nodeDragThreshold={2}
        onNodeClick={(_, n) => onNodeClick?.(n.id)}
        onEdgeClick={(_, e) => onEdgeClick?.({ source: e.source, target: e.target })}
        proOptions={{ hideAttribution: true }}
      >
        {showBackground && (
          <Background variant={BackgroundVariant.Dots} gap={18} color={palette['--surface-muted']} />
        )}
        <Controls
          position="bottom-right"
          className="!border !border-bd-1 !bg-bg-1 !shadow-sm [&>button]:!border-bd-0 [&>button]:!bg-bg-1 [&>button]:!fill-current [&>button]:!text-tx-1 [&>button:hover]:!bg-bg-3"
          showInteractive={false}
        />
        {showMiniMap && (
          <MiniMap nodeColor={() => palette['--accent']} maskColor="rgba(0,0,0,0.4)" />
        )}
      </ReactFlow>
    </div>
  );
}

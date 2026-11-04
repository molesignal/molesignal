import { changesInPlace, isStreamSink, type PipelineGraphModel } from '../PipelineGraph/model';

type NodeKind = 'source' | 'transform' | 'sink';

export function parseNodeId(id: string): { kind: NodeKind; index: number } | null {
  const match = /^(source|transform|sink)-(\d+)$/.exec(id);

  return match ? { kind: match[1] as NodeKind, index: Number(match[2]) } : null;
}

export function canRemoveNode(model: PipelineGraphModel, id: string): boolean {
  const node = parseNodeId(id);
  if (!node) return false;
  if (node.kind === 'transform') return node.index < model.transforms.length;
  if (model.mode === 'alert') return false;
  if (node.kind === 'source') return node.index < model.sources.length && model.sources.length > 1;
  const sink = model.sinks[node.index];
  return !changesInPlace(model) && sink !== undefined &&
    (!isStreamSink(sink) || model.sinks.filter(isStreamSink).length > 1);
}

/** Preserve the selected node when an earlier sibling is removed. */
export function removeNode(model: PipelineGraphModel, id: string, selectedId: string) {
  const node = parseNodeId(id);
  if (!node || !canRemoveNode(model, id)) return { model, selectedId };
  const field = node.kind === 'source' ? 'sources' : node.kind === 'sink' ? 'sinks' : 'transforms';
  const next = { ...model, [field]: model[field].filter((_, index) => index !== node.index) };
  const selected = parseNodeId(selectedId);
  if (selected?.kind !== node.kind) return { model: next, selectedId };
  if (selected.index > node.index) selectedId = `${node.kind}-${selected.index - 1}`;
  else if (selected.index === node.index) {
    selectedId = next[field].length === 0 ? 'source-0' : `${node.kind}-${Math.max(0, node.index - 1)}`;
  }
  return { model: next, selectedId };
}

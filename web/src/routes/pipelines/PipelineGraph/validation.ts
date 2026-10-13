import {
  connectorOf,
  defaultRouting,
  writtenSinkIndex,
  type PipelineGraphModel,
  type TransformStep,
} from './model';
import { isStreamName, isStreamPrefix } from './streamName';
import { processorProblem } from '../processors/catalog';

export type GraphIssueLevel = 'error' | 'warning';

export interface GraphIssue {
  level: GraphIssueLevel;
  /** i18n code（`graph.validation.<code>`），UI 侧渲染。 */
  code: string;
  params?: Record<string, string | number>;
  /** The node the issue is about (`source-0`, `transform-1`, `sink-0`), when it is about one. */
  node?: string;
  /**
   * An error that is only something not filled in yet. It holds a save back
   * like any other, but is shown as work to do, not as a mistake, until a
   * save is tried.
   */
  pending?: true;
}

/** Whether a flow with these issues can be saved. */
export function blocksSave(issues: readonly GraphIssue[]): boolean {
  return issues.some((issue) => issue.level === 'error');
}

/**
 * 流水线图校验：返回错误/警告列表（`code` 在 UI 侧用 i18n 渲染）。
 *
 * An error is something the backend would reject (`scheduled_pipelines/
 * validation.rs`, `domain/pipeline/realtime.rs`), a run would fail on
 * (`infra/pipeline/exec/processing.rs`), or the save would have to make up. A
 * warning is saved as it is: it says what the pipeline will not do.
 */
export function validateGraph(
  model: PipelineGraphModel,
  connectorIds: string[] = [],
): GraphIssue[] {
  const issues: GraphIssue[] = [];
  const scheduled = model.mode !== 'realtime';
  const sources = model.sources.map((item) => item.trim());
  const sinks = model.sinks.map((item) => item.trim());
  const routed = model.transforms.some((step) => step.kind === 'builtin');
  const read = sources[0] ?? '';
  const written = writtenSinkIndex(sinks);

  if (sources.length === 0) issues.push({ level: 'error', code: 'no_source' });
  // Every destination a connector: nothing says which stream the events are written to.
  if (written === -1 && !routed) issues.push({ level: 'error', code: 'no_sink' });

  sources.forEach((source, index) => {
    const node = `source-${index}`;

    if (index > 0) {
      issues.push({ level: 'warning', code: 'source_unread', params: { name: source, read }, node });
    } else if (!source) {
      issues.push({ level: 'error', code: 'source_missing', node, pending: true });
    } else if (!isStreamName(source)) {
      issues.push({ level: 'error', code: 'stream_name_invalid', params: { name: source }, node });
    }
  });

  const connectors = new Set<string>();
  sinks.forEach((sink, index) => {
    const node = `sink-${index}`;
    const connector = connectorOf(sink);

    if (connector !== null) {
      if (!connector) {
        issues.push({ level: 'error', code: 'connector_unset', node, pending: true });
      } else if (connectors.has(connector)) {
        issues.push({ level: 'warning', code: 'connector_duplicate', node });
      } else if (connectorIds.length > 0 && !connectorIds.includes(connector)) {
        issues.push({ level: 'warning', code: 'connector_missing', params: { id: connector }, node });
      }
      connectors.add(connector);
      return;
    }

    if (index !== written) {
      issues.push({
        level: 'warning',
        code: 'sink_unwritten',
        params: { name: sink, written: sinks[written] ?? '' },
        node,
      });
      return;
    }
    // A routing step decides where events go; the destination node only stands for that.
    if (routed) return;

    if (!sink) {
      issues.push({ level: 'error', code: 'sink_missing', node, pending: true });
    } else if (!isStreamName(sink)) {
      issues.push({ level: 'error', code: 'stream_name_invalid', params: { name: sink }, node });
    } else if (scheduled && sink === read) {
      issues.push({ level: 'error', code: 'source_is_sink', params: { name: sink }, node });
    }
  });

  if (!scheduled) {
    if (model.signalType !== 'logs') issues.push({ level: 'error', code: 'realtime_logs' });
    if (sources.length !== 1 || sinks.length !== 1 || connectors.size > 0) {
      issues.push({ level: 'error', code: 'realtime_nodes' });
    }
  }

  model.transforms.forEach((step, index) => {
    const node = `transform-${index}`;
    const name = step.name.trim() || String(index + 1);

    if (!step.name.trim()) {
      issues.push({
        level: 'warning',
        code: 'transform_name_missing',
        params: { index: index + 1 },
        node,
      });
    }

    const issue = step.kind === 'builtin' ? routeIssue(step, scheduled ? sources : []) : scriptIssue(step);
    if (issue) issues.push({ level: 'error', ...issue, params: { name, ...issue.params }, node });
  });

  return issues;
}

type StepIssue = Pick<GraphIssue, 'code' | 'params' | 'pending'>;

/**
 * What is wrong with a routing step. `sources` are the streams a scheduled
 * run reads: routing an event back into one of them fails the whole run.
 */
function routeIssue(step: TransformStep, sources: readonly string[]): StepIssue | null {
  const route = step.routing ?? defaultRouting;
  const stream = (route.kind === 'fixed' ? step.target : route.fallback)?.trim() ?? '';

  if (!stream || (route.kind === 'field' && !route.field.trim())) {
    return { code: route.kind === 'fixed' ? 'route_target_missing' : 'route_field_missing', pending: true };
  }
  if (!isStreamName(stream)) return { code: 'route_stream_invalid', params: { stream } };
  if (route.kind === 'field' && !isStreamPrefix(route.prefix)) return { code: 'route_prefix_invalid' };
  if (sources.includes(stream)) return { code: 'route_to_source', params: { stream } };

  return null;
}

function scriptIssue(step: TransformStep): StepIssue | null {
  const problem = step.processor ? processorProblem(step.processor) : null;

  if (problem) return { code: problem, ...(problem !== 'processor_number_invalid' && { pending: true }) };
  if (!step.script.trim()) return { code: 'transform_script_missing', pending: true };

  return null;
}

/**
 * How many nodes a pipeline has and how many links join them: the source into
 * the first step, the steps in a chain, the last one into every destination.
 * Names that are saved but neither read nor written are not part of it.
 */
export function pipelineGraphStats(model: PipelineGraphModel): {
  nodes: number;
  edges: number;
} {
  const written = writtenSinkIndex(model.sinks);
  const destinations = model.sinks.filter(
    (sink, index) => connectorOf(sink) !== null || index === written,
  ).length;
  const sources = Math.min(1, model.sources.length);

  return {
    nodes: sources + model.transforms.length + destinations,
    edges: sources === 0 ? 0 : model.transforms.length + destinations,
  };
}

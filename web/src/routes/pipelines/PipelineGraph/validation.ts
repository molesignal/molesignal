import {
  connectorOf,
  defaultRouting,
  firstStreamIndex,
  isRouted,
  MAX_STREAMS,
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
 * validation.rs`, `domain/pipeline/scheduled.rs`, `domain/pipeline/
 * realtime.rs`), a run would fail on (`infra/pipeline/exec/processing.rs`),
 * or the save would have to make up. A warning is saved as it is: it says
 * what the pipeline will do differently from how it looks.
 */
export function validateGraph(
  model: PipelineGraphModel,
  connectorIds: string[] = [],
): GraphIssue[] {
  const issues: GraphIssue[] = [];
  const scheduled = model.mode !== 'realtime';
  const sources = model.sources.map((item) => item.trim());
  const sinks = model.sinks.map((item) => item.trim());
  const routed = isRouted(model);
  const first = firstStreamIndex(sinks);
  const streams = sinks.filter((sink) => connectorOf(sink) === null);
  const read = new Set(sources.filter(Boolean));

  if (sources.length === 0) issues.push({ level: 'error', code: 'no_source' });
  // Every destination a connector: nothing says which stream the events are written to.
  if (first === -1 && !routed) issues.push({ level: 'error', code: 'no_sink' });

  sources.forEach((source, index) => {
    const node = `source-${index}`;

    if (!source) {
      issues.push({ level: 'error', code: 'source_missing', node, pending: true });
    } else if (!isStreamName(source)) {
      issues.push({ level: 'error', code: 'stream_name_invalid', params: { name: source }, node });
    } else if (sources.indexOf(source) < index) {
      issues.push({ level: 'warning', code: 'source_duplicate', params: { name: source }, node });
    }
  });

  if (sources.length > MAX_STREAMS) {
    issues.push({ level: 'error', code: 'too_many_sources', params: { max: MAX_STREAMS } });
  }

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

    // A routing step decides where the first stream's events go; that node only stands for it.
    if (routed && index === first) return;

    if (!sink) {
      issues.push({ level: 'error', code: 'sink_missing', node, pending: true });
    } else if (!isStreamName(sink)) {
      issues.push({ level: 'error', code: 'stream_name_invalid', params: { name: sink }, node });
    } else if (scheduled && read.has(sink)) {
      issues.push({ level: 'error', code: 'source_is_sink', params: { name: sink }, node });
    } else if (sinks.indexOf(sink) < index) {
      issues.push({ level: 'warning', code: 'sink_duplicate', params: { name: sink }, node });
    }
  });

  if (streams.length > MAX_STREAMS) {
    issues.push({ level: 'error', code: 'too_many_sinks', params: { max: MAX_STREAMS } });
  }

  if (!scheduled) {
    if (model.signalType !== 'logs') issues.push({ level: 'error', code: 'realtime_logs' });
    if (sources.length !== 1 || streams.length !== 1 || connectors.size > 0) {
      issues.push({ level: 'error', code: 'realtime_nodes' });
    }
  }

  const deadLetters = model.deadLetterStream.trim();

  if (scheduled && deadLetters) {
    if (!isStreamName(deadLetters)) {
      issues.push({ level: 'error', code: 'dead_letter_invalid', params: { name: deadLetters } });
    } else if (read.has(deadLetters) || streams.includes(deadLetters)) {
      issues.push({ level: 'error', code: 'dead_letter_conflict', params: { name: deadLetters } });
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

  // A value that is there but wrong is a mistake; one that is not there yet is work to do.
  const mistake = problem === 'processor_number_invalid' || problem === 'processor_percent_invalid';

  if (problem) return { code: problem, ...(!mistake && { pending: true }) };
  if (!step.script.trim()) return { code: 'transform_script_missing', pending: true };

  return null;
}

/**
 * How many nodes a pipeline has and how many links join them: every source
 * into the first step, the steps in a chain, the last one into every
 * destination. Without steps, every source leads into every destination.
 */
export function pipelineGraphStats(model: PipelineGraphModel): {
  nodes: number;
  edges: number;
} {
  const sources = model.sources.length;
  const steps = model.transforms.length;
  const destinations = model.sinks.length;

  return {
    nodes: sources + steps + destinations,
    edges: steps === 0 ? sources * destinations : sources + steps - 1 + destinations,
  };
}

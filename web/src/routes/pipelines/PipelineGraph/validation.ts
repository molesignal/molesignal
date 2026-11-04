import { isPlatformStream } from '@/product/platformStreams';

import {
  changesInPlace,
  connectorOf,
  defaultRouting,
  firstStreamIndex,
  goesOutside,
  isRouted,
  isStreamSink,
  MAX_STREAMS,
  tableOf,
  type PipelineGraphModel,
  type TransformStep,
} from './model';
import { isProtectedField, writtenFields } from './protectedFields';
import {
  isRumStream,
  isStreamName,
  isStreamPrefix,
  isWritableStream,
  keepsEveryRow,
} from './streamName';
import { countingIssue, countingKinds, isCountingKind } from '../counting/model';
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
  if (model.mode === 'alert') return validateAlertPipeline(model);

  const issues: GraphIssue[] = [];
  const scheduled = model.mode !== 'realtime';
  // Metrics and spans changed as they arrive stay in their stream: there is no destination to check.
  const inPlace = changesInPlace(model);
  const sources = model.sources.map((item) => item.trim());
  const sinks = model.sinks.map((item) => item.trim());
  const routed = isRouted(model);
  const first = firstStreamIndex(sinks);
  const streams = sinks.filter(isStreamSink);
  const read = new Set(sources.filter(Boolean));
  const tables = new Set<string>();

  if (sources.length === 0) issues.push({ level: 'error', code: 'no_source' });
  // Every destination a connector: nothing says which stream the events are written to.
  if (first === -1 && !routed && !inPlace) issues.push({ level: 'error', code: 'no_sink' });

  sources.forEach((source, index) => {
    const node = `source-${index}`;

    if (!source) {
      issues.push({ level: 'error', code: 'source_missing', node, pending: true });
    } else if (!isStreamName(source)) {
      issues.push({ level: 'error', code: 'stream_name_invalid', params: { name: source }, node });
    } else if (!scheduled && source === 'rum_sessions') {
      // Sessions are built from the views as they arrive: no pipeline changes them.
      issues.push({ level: 'error', code: 'rum_sessions_source', params: { name: source }, node });
    } else if (!scheduled && isPlatformStream(source)) {
      issues.push({ level: 'error', code: 'platform_source', params: { name: source }, node });
    } else if (sources.indexOf(source) < index) {
      issues.push({ level: 'warning', code: 'source_duplicate', params: { name: source }, node });
    }
  });

  if (sources.length > MAX_STREAMS) {
    issues.push({ level: 'error', code: 'too_many_sources', params: { max: MAX_STREAMS } });
  }

  const connectors = new Set<string>();
  sinks.forEach((sink, index) => {
    if (inPlace) return;

    const node = `sink-${index}`;
    const connector = connectorOf(sink);
    const table = tableOf(sink);

    if (table !== null) {
      if (!table) {
        issues.push({ level: 'error', code: 'table_unset', node, pending: true });
      } else if (tables.has(table)) {
        issues.push({ level: 'warning', code: 'table_duplicate', params: { name: table }, node });
      }
      tables.add(table);
      return;
    }

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
    } else if (!isWritableStream(sink)) {
      issues.push({ level: 'error', code: unwritableCode(sink), params: { name: sink }, node });
    } else if (scheduled && read.has(sink)) {
      issues.push({ level: 'error', code: 'source_is_sink', params: { name: sink }, node });
    } else if (sinks.indexOf(sink) < index) {
      issues.push({ level: 'warning', code: 'sink_duplicate', params: { name: sink }, node });
    }
  });

  if (streams.length > MAX_STREAMS) {
    issues.push({ level: 'error', code: 'too_many_sinks', params: { max: MAX_STREAMS } });
  }

  if (inPlace) {
    if (sources.length !== 1) issues.push({ level: 'error', code: 'realtime_nodes' });
    if (routed) issues.push({ level: 'error', code: 'in_place_route' });
  } else if (!scheduled) {
    if (sources.length !== 1 || streams.length !== 1 || connectors.size > 0 || tables.size > 0) {
      issues.push({ level: 'error', code: 'realtime_nodes' });
    }
  }

  const deadLetters = model.deadLetterStream.trim();

  if (scheduled && deadLetters) {
    if (!isStreamName(deadLetters)) {
      issues.push({ level: 'error', code: 'dead_letter_invalid', params: { name: deadLetters } });
    } else if (!isWritableStream(deadLetters)) {
      issues.push({ level: 'error', code: unwritableCode(deadLetters), params: { name: deadLetters } });
    } else if (read.has(deadLetters) || streams.includes(deadLetters)) {
      issues.push({ level: 'error', code: 'dead_letter_conflict', params: { name: deadLetters } });
    }
  }

  const counting = countingKinds(model.mode, model.signalType, sources[0] ?? '');
  // What a step that keeps count counted is kept under its name: two such steps cannot share one.
  const countingNames = model.transforms
    .filter((step) => isCountingKind(step.kind))
    .map((step) => step.name.trim());

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

    const source = sources[0] ?? '';
    const issue = step.kind === 'builtin'
      ? routeIssue(step, scheduled ? sources : [])
      : isCountingKind(step.kind)
        ? countingIssue(step.kind, step, counting, source, model.mode)
        : scriptIssue(step);
    if (issue) issues.push({ level: 'error', ...issue, params: { name, ...issue.params }, node });

    if (isCountingKind(step.kind) && step.name.trim() && countingNames.filter((other) => other === step.name.trim()).length > 1) {
      issues.push({ level: 'error', code: 'counting_name_duplicate', params: { name }, node });
    }

    // The backend fails every metric, span or RUM event such a step changes; VRL written by hand
    // is tried when the pipeline is saved.
    const kept = inPlace && step.processor
      ? writtenFields(step.processor).find((field) => isProtectedField(model.signalType, field, source))
      : undefined;
    if (kept) issues.push({ level: 'error', code: 'protected_field', params: { name, field: kept }, node });

    // A filter or a sample drops events, which a pipeline on RUM views or vitals never does.
    const drops = step.processor?.op === 'filter' || step.processor?.op === 'sample';
    if (!scheduled && keepsEveryRow(source) && drops) {
      issues.push({ level: 'error', code: 'rum_keeps_rows', params: { name, source }, node });
    }
  });

  return issues;
}

/** Why a pipeline may not write this stream: RUM intake or the platform writes it. */
function unwritableCode(name: string): string {
  return isRumStream(name) ? 'rum_written' : 'platform_written';
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
  if (!isWritableStream(stream)) {
    return { code: isRumStream(stream) ? 'route_to_rum' : 'route_to_platform', params: { stream } };
  }
  if (sources.includes(stream)) return { code: 'route_to_source', params: { stream } };

  return null;
}

/**
 * An alert pipeline reads alert events and writes them back, so it has no
 * stream to check: only its steps. A routing step has nowhere to send an alert
 * event, and an AI summary needs to say what it asks.
 */
function validateAlertPipeline(model: PipelineGraphModel): GraphIssue[] {
  const issues: GraphIssue[] = [];

  model.transforms.forEach((step, index) => {
    const node = `transform-${index}`;
    const name = step.name.trim() || String(index + 1);

    if (!step.name.trim()) {
      issues.push({ level: 'warning', code: 'transform_name_missing', params: { index: index + 1 }, node });
    }

    const issue: StepIssue | null =
      step.kind === 'builtin'
        ? { code: 'alert_route' }
        : isCountingKind(step.kind)
          ? { code: 'counting_realtime_only' }
        : model.alert.stage === 'notify' && goesOutside(step)
          ? { code: 'alert_notify_outside' }
          : step.kind === 'ai_summary'
            ? (step.prompt ?? '').trim()
              ? null
              : { code: 'alert_prompt_missing', pending: true }
            : step.kind === 'http_enrich'
              ? (step.connectorId ?? '').trim()
                ? null
                : { code: 'alert_connector_missing', pending: true }
              : step.kind === 'query_enrich'
                ? queryIssue(step)
                : step.processor?.op === 'sample'
                  ? { code: 'alert_sample' }
                  : scriptIssue(step);

    if (issue) issues.push({ level: 'error', ...issue, params: { name, ...issue.params }, node });
  });

  return issues;
}

/** What a query enrich step still lacks. */
function queryIssue(step: TransformStep): StepIssue | null {
  if (!(step.query?.stream ?? '').trim()) return { code: 'alert_query_stream_missing', pending: true };
  if (!(step.query?.statement ?? '').trim()) return { code: 'alert_query_statement_missing', pending: true };

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
  // An alert pipeline's one source and one destination are not in the lists.
  const alert = model.mode === 'alert';
  const sources = alert ? 1 : model.sources.length;
  const steps = model.transforms.length;
  const destinations = alert ? 1 : model.sinks.length;

  return {
    nodes: sources + steps + destinations,
    edges: steps === 0 ? sources * destinations : sources + steps - 1 + destinations,
  };
}

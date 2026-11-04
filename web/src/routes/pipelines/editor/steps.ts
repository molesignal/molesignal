import {
  ArrowRightLeft,
  Braces,
  CalendarClock,
  CaseSensitive,
  ChartLine,
  Code2,
  Equal,
  Eraser,
  EyeOff,
  FileSearch,
  Filter,
  ListChecks,
  PenLine,
  Percent,
  Regex,
  Replace,
  ReplaceAll,
  Sparkles,
  Split,
  TableProperties,
  Timer,
  Webhook,
  type LucideIcon,
} from 'lucide-react';

import { defaultCounting, isCountingKind } from '../counting/model';
import {
  DEFAULT_SUMMARY_ANNOTATION,
  defaultEnrichQuery,
  defaultRouting,
  stepType,
  type PipelineMode,
  type StepType,
  type TransformStep,
} from '../PipelineGraph/model';
import {
  compileProcessor,
  defaultProcessor,
  processorStepName,
} from '../processors/catalog';

export const STEP_ICONS: Record<StepType, LucideIcon> = {
  route: Split,
  vrl: Code2,
  parse_json: Braces,
  parse_key_value: Equal,
  parse_regex: Regex,
  rename: ArrowRightLeft,
  rename_regex: ReplaceAll,
  remove: Eraser,
  keep: ListChecks,
  set: PenLine,
  mask: EyeOff,
  case: CaseSensitive,
  convert: Replace,
  filter: Filter,
  sample: Percent,
  lookup: TableProperties,
  ai_summary: Sparkles,
  http_enrich: Webhook,
  query_enrich: FileSearch,
  generate_metric: ChartLine,
  quota: CalendarClock,
  throttle: Timer,
};

/**
 * What a hand-written step starts as: a note on what goes here. VRL takes a
 * script of comments alone, and it leaves the event as it is.
 */
export const STARTER_VRL = `# \`.\` is the event. Assign to its fields to change it, for example:
# .service = "checkout"`;

/** What an AI summary step asks the model until someone writes their own. */
export const STARTER_PROMPT =
  'Summarize this alert for the person on call in two or three sentences: what is failing, where, and what to check first.';

/** The name a step of this kind is given until someone names it. */
export function defaultStepName(type: StepType): string {
  if (type === 'route') return 'route';
  if (type === 'vrl') return 'transform';
  if (type === 'ai_summary') return 'ai-summary';
  if (type === 'http_enrich') return 'call-api';
  if (type === 'query_enrich') return 'query';
  if (type === 'generate_metric') return 'metric';
  if (isCountingKind(type)) return type;
  return processorStepName(type);
}

/** `base`, or `base-2`, `base-3`… when a step of that name is already there. */
function uniqueName(base: string, taken: readonly string[]): string {
  if (!taken.includes(base)) return base;

  let suffix = 2;
  while (taken.includes(`${base}-${suffix}`)) suffix += 1;
  return `${base}-${suffix}`;
}

/** A new step of a kind, ready to be filled in. `mode` and `source` are the pipeline's. */
export function newStep(
  type: StepType,
  taken: readonly string[],
  mode?: PipelineMode,
  source?: string,
): TransformStep {
  return ofType(type, uniqueName(defaultStepName(type), taken), mode, source);
}

/**
 * The same step as another kind. A name nobody chose follows the kind; one
 * that was typed stays.
 */
export function retypeStep(
  step: TransformStep,
  type: StepType,
  taken: readonly string[],
  mode?: PipelineMode,
  source?: string,
): TransformStep {
  const previous = defaultStepName(stepType(step));
  const untouched = step.name.trim() === '' || new RegExp(`^${previous}(-\\d+)?$`).test(step.name);
  const others = taken.filter((name) => name !== step.name);
  const next = ofType(type, untouched ? uniqueName(defaultStepName(type), others) : step.name, mode, source);

  // Hand-written VRL keeps its script when it is only switched away and back.
  return type === 'vrl' && step.kind !== 'builtin' && step.script.trim()
    ? { ...next, script: step.script }
    : next;
}

function ofType(type: StepType, name: string, mode?: PipelineMode, source = ''): TransformStep {
  if (isCountingKind(type)) return { name, kind: type, script: '', ...defaultCounting(type, source.trim()) };
  if (type === 'route') {
    return {
      name,
      kind: 'builtin',
      operation: 'route',
      script: '',
      // Routing by a field is what a routing step is for; which field is for its author to say.
      routing: { ...defaultRouting, kind: 'field', field: '' },
      target: '',
    };
  }
  if (type === 'vrl') return { name, kind: 'vrl', script: STARTER_VRL };
  if (type === 'ai_summary') {
    return { name, kind: 'ai_summary', script: '', prompt: STARTER_PROMPT, annotation: DEFAULT_SUMMARY_ANNOTATION };
  }
  if (type === 'http_enrich') return { name, kind: 'http_enrich', script: '', connectorId: '' };
  if (type === 'query_enrich') return { name, kind: 'query_enrich', script: '', query: { ...defaultEnrichQuery } };

  const fresh = defaultProcessor(type);
  // What these two go through in an alert event is its labels, as Prometheus's relabeling does.
  const processor =
    mode === 'alert' && (fresh.op === 'rename_regex' || fresh.op === 'keep') ? { ...fresh, within: 'labels' } : fresh;

  return { name, kind: 'vrl', processor, script: compileProcessor(processor) };
}

/** A step moved one place earlier or later; the list as it was when there is nowhere to go. */
export function moveStep(
  steps: readonly TransformStep[],
  index: number,
  direction: -1 | 1,
): TransformStep[] {
  const target = index + direction;
  const moved = steps[index];
  const swapped = steps[target];
  if (!moved || !swapped) return [...steps];

  const next = [...steps];
  next[index] = swapped;
  next[target] = moved;
  return next;
}

import {
  ArrowRightLeft,
  Braces,
  CaseSensitive,
  Code2,
  Equal,
  Eraser,
  EyeOff,
  Filter,
  PenLine,
  Percent,
  Regex,
  Replace,
  Split,
  type LucideIcon,
} from 'lucide-react';

import {
  defaultRouting,
  stepType,
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
  remove: Eraser,
  set: PenLine,
  mask: EyeOff,
  case: CaseSensitive,
  convert: Replace,
  filter: Filter,
  sample: Percent,
};

/**
 * What a hand-written step starts as: a note on what goes here. VRL takes a
 * script of comments alone, and it leaves the event as it is.
 */
export const STARTER_VRL = `# \`.\` is the event. Assign to its fields to change it, for example:
# .service = "checkout"`;

/** The name a step of this kind is given until someone names it. */
export function defaultStepName(type: StepType): string {
  if (type === 'route') return 'route';
  if (type === 'vrl') return 'transform';
  return processorStepName(type);
}

/** `base`, or `base-2`, `base-3`… when a step of that name is already there. */
function uniqueName(base: string, taken: readonly string[]): string {
  if (!taken.includes(base)) return base;

  let suffix = 2;
  while (taken.includes(`${base}-${suffix}`)) suffix += 1;
  return `${base}-${suffix}`;
}

/** A new step of a kind, ready to be filled in. */
export function newStep(type: StepType, taken: readonly string[]): TransformStep {
  return ofType(type, uniqueName(defaultStepName(type), taken));
}

/**
 * The same step as another kind. A name nobody chose follows the kind; one
 * that was typed stays.
 */
export function retypeStep(
  step: TransformStep,
  type: StepType,
  taken: readonly string[],
): TransformStep {
  const previous = defaultStepName(stepType(step));
  const untouched = step.name.trim() === '' || new RegExp(`^${previous}(-\\d+)?$`).test(step.name);
  const others = taken.filter((name) => name !== step.name);
  const next = ofType(type, untouched ? uniqueName(defaultStepName(type), others) : step.name);

  // Hand-written VRL keeps its script when it is only switched away and back.
  return type === 'vrl' && step.kind !== 'builtin' && step.script.trim()
    ? { ...next, script: step.script }
    : next;
}

function ofType(type: StepType, name: string): TransformStep {
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

  const processor = defaultProcessor(type);
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

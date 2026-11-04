import type { AnalyticsStep, AnalyticsStepKind, RumFilter } from '@/api/rum';

/** The fields a step can look at, by what it looks at. */
export const STEP_FIELDS: Record<AnalyticsStepKind, readonly string[]> = {
  views: ['view.url_path_group', 'view.url_path', 'view.name'],
  actions: ['action.target.name', 'action.target.selector', 'action.type'],
};

/** How a step compares: the value is exactly the text, or it holds it. */
export type StepOp = '=' | 'contains';

/** One step as the builder holds it: what it looks at, in which field, and for what. */
export interface StepDraft {
  kind: AnalyticsStepKind;
  field: string;
  op: StepOp;
  value: string;
}

/** The most steps a funnel has, as the server allows. */
export const MAX_STEPS = 8;
export const MIN_STEPS = 2;

/** The field a step of a kind begins with: the page of a browser, the screen of an app. */
export function defaultField(kind: AnalyticsStepKind, browser: boolean): string {
  if (kind === 'actions') return 'action.target.name';

  return browser ? 'view.url_path_group' : 'view.name';
}

export function emptyStep(kind: AnalyticsStepKind, browser: boolean): StepDraft {
  return { kind, field: defaultField(kind, browser), op: '=', value: '' };
}

/** A step is ready to ask when it names what to look for. */
export const isComplete = (step: StepDraft): boolean => step.value.trim() !== '';

/** The step as the server takes it. */
export function toStep(step: StepDraft): AnalyticsStep {
  const filters: RumFilter[] = [{ field: step.field, op: step.op, value: step.value.trim() }];

  return { type: step.kind, filters };
}

// ---------------------------------------------------------------------------------------------
// The steps in an address: `v:view.url_path_group=%2Fcheckout|a:action.target.name~Pay`. A step is
// its kind (`v` views, `a` actions), its field, `=` or `~` (contains) and its value, escaped.

const KIND_CODE: Record<AnalyticsStepKind, string> = { views: 'v', actions: 'a' };

export function encodeSteps(steps: readonly StepDraft[]): string {
  return steps
    .map((step) => `${KIND_CODE[step.kind]}:${step.field}${step.op === '=' ? '=' : '~'}${encodeURIComponent(step.value)}`)
    .join('|');
}

/** The steps an address says; a part that is not a step is left out. */
export function decodeSteps(text: string | null): StepDraft[] {
  if (!text) return [];

  return text
    .split('|')
    .slice(0, MAX_STEPS)
    .flatMap((part): StepDraft[] => {
      const match = /^([va]):([a-z_.]+)([=~])(.*)$/.exec(part);

      if (!match) return [];

      const kind: AnalyticsStepKind = match[1] === 'v' ? 'views' : 'actions';
      const field = match[2] as string;

      if (!STEP_FIELDS[kind].includes(field)) return [];

      try {
        return [{ kind, field, op: match[3] === '=' ? '=' : 'contains', value: decodeURIComponent(match[4] as string) }];
      } catch {
        return [];
      }
    });
}

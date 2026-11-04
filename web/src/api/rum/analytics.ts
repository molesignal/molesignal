import { getOrPost } from '@/lib/http';

import type { RumFilter } from './explorer';

/** What a step of a funnel looks at: a page or screen that was opened, or something a person did. */
export type AnalyticsStepKind = 'views' | 'actions';

export interface AnalyticsStep {
  type: AnalyticsStepKind;
  /** What the answer calls the step; the value it looks for when left out. */
  name?: string;
  filters: RumFilter[];
}

interface Range {
  from?: number;
  to?: number;
}

export interface FunnelParams extends Range {
  /** What every step has to meet: the application, the environment… */
  filters?: RumFilter[];
  steps: AnalyticsStep[];
  /** The longest from the first step to a later one. */
  windowSecs?: number;
  /** A text field of the first step's rows to count the sessions by. */
  breakdown?: string;
}

export interface FunnelStep {
  name: string;
  sessions: number;
  /** The share of the sessions of the first step that got here. */
  conversion: number | null;
  /** The share of the sessions of the step before that got here. */
  fromPrevious: number | null;
  /** The middle of the times between the step before and this one. */
  medianMs: number | null;
}

export interface FunnelGroup {
  value: string | null;
  steps: FunnelStep[];
}

interface Answered {
  /** The range the answer is of, in microseconds. */
  from: number;
  to: number;
  unavailable?: 'time_budget' | 'busy' | 'timestamp_masked';
}

export interface Funnel extends Answered {
  groups: FunnelGroup[];
}

export interface PathsParams extends Range {
  filters?: RumFilter[];
  /** The page the paths start from, or end at going backward. */
  anchor?: string;
  backward?: boolean;
  /** How many pages after (before) the anchor, 1 to 6. */
  depth?: number;
  /** The field a page is told by. */
  by?: string;
}

export interface PathNode {
  column: number;
  page: string;
  sessions: number;
  /** Sessions that went no further (or, going backward, began here). */
  ends: number;
}

export interface PathLink {
  column: number;
  source: string;
  target: string;
  sessions: number;
}

/** What the pages beyond the most visited of a column are called in an answer. */
export const PATHS_OTHER = '__other__';

export interface Paths extends Answered {
  columns: number;
  sessions: number;
  backward: boolean;
  nodes: PathNode[];
  links: PathLink[];
}

export type RetentionPeriod = 'day' | 'week';

export interface RetentionParams extends Range {
  filters?: RumFilter[];
  period?: RetentionPeriod;
  /** How many periods after the first, 1 to 30. */
  periods?: number;
}

export interface RetentionCohort {
  /** When the period the people were first seen in begins, in microseconds. */
  startTime: number;
  users: number;
  /** How many were seen 1, 2, … periods later; none for a period that has not ended. */
  returned: Array<number | null>;
}

export interface Retention extends Answered {
  period: RetentionPeriod;
  periods: number;
  cohorts: RetentionCohort[];
}

const json = (list: unknown[] | undefined) => (list && list.length > 0 ? JSON.stringify(list) : undefined);

/** How many sessions did a series of steps in order. */
export async function funnel(params: FunnelParams, signal?: AbortSignal): Promise<Funnel> {
  const { filters, steps, ...rest } = params;

  return getOrPost<Funnel>(
    '/rum/analytics/funnel',
    { params: { ...rest, filters: json(filters), steps: json(steps) }, body: params },
    { ...(signal && { signal }) },
  );
}

/** Where sessions went from a page, or before it. */
export async function paths(params: PathsParams, signal?: AbortSignal): Promise<Paths> {
  const { filters, ...rest } = params;

  return getOrPost<Paths>(
    '/rum/analytics/paths',
    { params: { ...rest, filters: json(filters) }, body: params },
    { ...(signal && { signal }) },
  );
}

/** How many of the people first seen in a period were seen again in the periods after. */
export async function retention(params: RetentionParams, signal?: AbortSignal): Promise<Retention> {
  const { filters, ...rest } = params;

  return getOrPost<Retention>(
    '/rum/analytics/retention',
    { params: { ...rest, filters: json(filters) }, body: params },
    { ...(signal && { signal }) },
  );
}

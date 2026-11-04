import { http } from '@/lib/http';

import type { RumFilter } from './explorer';

/**
 * What is being done about an issue. `regressed` is set by the rules, when a resolved issue is
 * seen again in a version newer than the one it was resolved in; a person sets the others.
 */
export type RumIssueStatus = 'open' | 'investigating' | 'regressed' | 'resolved' | 'ignored';

/** The states a person can set. */
export const SETTABLE_STATUSES: readonly RumIssueStatus[] = ['open', 'investigating', 'resolved', 'ignored'];

/** An issue as kept: what it is, who has it, and when and in which versions it was seen. */
export interface RumIssue {
  /** A hash of what the errors of the issue have in common. */
  id: string;
  applicationId: string;
  status: RumIssueStatus;
  assigneeId: string | null;
  /** The platform team that has it. */
  teamId: string | null;
  /** The errors of an application past the issues it may open in an hour, counted together. */
  overflow: boolean;
  /** Where the error comes from: `source`, `network`, `custom`… */
  source: string;
  errorType: string;
  message: string;
  /** The first frame of the application's own code, as `file:line · function`. */
  location: string;
  firstSeenAt: number;
  lastSeenAt: number;
  firstVersion: string | null;
  lastVersion: string | null;
  resolvedAt: number | null;
  /** The newest version the application had when the issue was resolved. */
  resolvedVersion: string | null;
  resolvedBy: string | null;
  regressedAt: number | null;
  regressedVersion: string | null;
  /** The environment of the error that brought the issue back; none when that error names none. */
  regressedEnvironment: string | null;
  updatedBy: string | null;
  updatedAt: number;
}

/** What the errors of an issue come to in the range asked for. */
export interface RumIssueNumbers {
  sessions: number;
  users: number;
  occurrences: number;
}

export interface RumIssueRow extends RumIssue, RumIssueNumbers {
  /** Occurrences in each of the equal parts the range is cut into, oldest first. */
  trend: number[];
}

export type RumIssueSort = 'sessions' | 'users' | 'occurrences' | 'last_seen' | 'first_seen' | 'status';

/** The states a list can be narrowed to: the unresolved ones, all, or one. */
export type RumIssueFilter = 'unresolved' | 'all' | RumIssueStatus;

export interface RumIssueListParams {
  application: string;
  env?: string;
  source?: string;
  /** More conditions on the errors counted, as the explorer says them. */
  filters?: RumFilter[];
  from?: number;
  to?: number;
  status?: RumIssueFilter;
  /** A user id, or `none`. */
  assignee?: string;
  /** A team id, or `none`. */
  team?: string;
  search?: string;
  /** Only the issues that are new in the range or went back in it. */
  fresh?: boolean;
  sort?: RumIssueSort;
  order?: 'asc' | 'desc';
  limit?: number;
}

export interface RumIssueList {
  issues: RumIssueRow[];
  /** How many issues the filters leave, before the list is cut to `limit`. */
  total: number;
  unavailable?: 'time_budget' | 'busy' | 'timestamp_masked';
}

/** One of the latest errors of an issue, as far as it says who met it and where. */
export interface RumIssueSample {
  timestamp: number;
  sessionId: string | null;
  userId: string | null;
  anonymousId: string | null;
  page: string | null;
  browser: string | null;
  system: string | null;
  version: string | null;
  hasReplay: boolean;
  /** The session was kept. The errors of a session that was not are kept alone: there is no session to open. */
  retained: boolean;
}

export interface RumFramePlace {
  file: string;
  function: string;
  line: number | null;
  column: number | null;
}

export interface RumFrame {
  /** What the SDK sent. */
  raw: RumFramePlace;
  /** What a source map, a mapping or a symbol file resolved it to. */
  restored: RumFramePlace | null;
  /** The application's own code, by the place the frame is shown at. */
  inApp: boolean;
}

export interface RumIssueStack {
  message: string | null;
  text: string | null;
  frames: RumFrame[];
  /** How far the frames were resolved; none when nothing was tried. */
  symbolication: 'completed' | 'partial' | 'missing' | null;
  handling: string | null;
  isCrash: boolean;
  category: string | null;
}

export interface RumIssueDetail extends RumIssue, RumIssueNumbers {
  /** The latest errors, newest first. */
  samples: RumIssueSample[];
  stack: RumIssueStack | null;
  unavailable?: 'time_budget' | 'busy' | 'timestamp_masked';
}

/** What a change says. A team or a person left out stays; `null` takes it off. */
export interface RumIssueChange {
  status?: Exclude<RumIssueStatus, 'regressed'>;
  assigneeId?: string | null;
  teamId?: string | null;
}

/** The issues of an application with errors in a range, with their numbers and a trend each. */
export async function listIssues(params: RumIssueListParams, signal?: AbortSignal): Promise<RumIssueList> {
  const { filters, ...rest } = params;
  const { data } = await http.get<RumIssueList>('/rum/issues', {
    params: { ...rest, ...(filters && filters.length > 0 ? { filters: JSON.stringify(filters) } : {}) },
    ...(signal && { signal }),
  });

  return data;
}

/** One issue, with its numbers in the range, its latest errors and the stack of the latest that has one. */
export async function getIssue(id: string, range: { from?: number; to?: number }, signal?: AbortSignal): Promise<RumIssueDetail> {
  const { data } = await http.get<RumIssueDetail>(`/rum/issues/${encodeURIComponent(id)}`, {
    params: range,
    ...(signal && { signal }),
  });

  return data;
}

export async function updateIssue(id: string, change: RumIssueChange): Promise<RumIssue> {
  const { data } = await http.patch<RumIssue>(`/rum/issues/${encodeURIComponent(id)}`, change);

  return data;
}

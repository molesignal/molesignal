import { http } from '@/lib/http';

import type { RumRow } from './explorer';

/** What a timeline item is: the kind of event, as the SDK sent it. */
export type RumEventType = 'view' | 'action' | 'error' | 'resource' | 'long_task' | 'vital' | 'log';

export interface TimelineItem extends RumRow {
  type: RumEventType;
}

export interface SessionTimeline {
  sessionId: string;
  /** The session itself, `null` while no row of it was derived yet. */
  session: RumRow | null;
  /** The rows of every type, oldest first. */
  items: TimelineItem[];
  /** The types that had more rows than a timeline carries. */
  truncated: string[];
}

/** Everything one session did, in the order it happened. */
export async function timeline(
  sessionId: string,
  range?: { from: number; to: number },
  signal?: AbortSignal,
): Promise<SessionTimeline> {
  const { data } = await http.get<SessionTimeline>(
    `/rum/sessions/${encodeURIComponent(sessionId)}/timeline`,
    { ...(range && { params: range }), ...(signal && { signal }) },
  );
  return data;
}

export interface ReplayEvent {
  type: string | number;
  timestamp?: number;
  ts?: number;
  [key: string]: unknown;
}

export interface SessionReplay {
  sessionId: string;
  segmentCount: number;
  events: ReplayEvent[];
}

export async function replay(sessionId: string): Promise<SessionReplay> {
  const { data } = await http.get<SessionReplay>(`/rum/replay/${encodeURIComponent(sessionId)}`);
  return data;
}

export interface RelatedTrace {
  traceId: string;
  service?: string | null;
  spanCount: number;
  durationMs?: number | null;
  startedAt?: number | null;
  /** `direct`: a request of the session carried the trace id. `time-correlated`: it ran while the session did. */
  relation: 'direct' | 'time-correlated';
}

export interface RelatedTraces {
  sessionId: string;
  primaryService?: string | null;
  traces: RelatedTrace[];
}

export async function relatedTraces(
  sessionId: string,
  range?: { from: number; to: number },
): Promise<RelatedTraces> {
  const { data } = await http.get<RelatedTraces>(
    `/rum/sessions/${encodeURIComponent(sessionId)}/related-traces`,
    range ? { params: range } : {},
  );
  return data;
}

/** The session whose request carried the id of a backend trace. */
export interface TraceSession {
  sessionId: string;
  timestamp: number;
  application?: string | null;
  page?: string | null;
  viewId?: string | null;
  actionId?: string | null;
  url?: string | null;
}

export async function traceSession(
  traceId: string,
  window?: { from: number; to: number },
): Promise<{ traceId: string; session: TraceSession | null }> {
  const { data } = await http.get<{ traceId: string; session: TraceSession | null }>(
    `/rum/traces/${encodeURIComponent(traceId)}/session`,
    window ? { params: window } : {},
  );
  return data;
}

/** A comment on the replay of a session, at a moment of it. */
export interface RumComment {
  id: string;
  application: string;
  sessionId: string;
  /** The moment it is about, in microseconds since the epoch. */
  atTime: number;
  body: string;
  authorId?: string | null;
  authorName?: string | null;
  createdAt: number;
  updatedAt: number;
  /** Whether the reader may change or remove it. */
  editable: boolean;
}

export async function comments(sessionId: string, signal?: AbortSignal): Promise<RumComment[]> {
  const { data } = await http.get<{ comments: RumComment[] }>(
    `/rum/sessions/${encodeURIComponent(sessionId)}/comments`,
    signal ? { signal } : {},
  );
  return data.comments;
}

export async function addComment(
  sessionId: string,
  comment: { application: string; atTime: number; body: string },
): Promise<RumComment> {
  const { data } = await http.post<RumComment>(`/rum/sessions/${encodeURIComponent(sessionId)}/comments`, comment);
  return data;
}

export async function changeComment(sessionId: string, commentId: string, body: string): Promise<RumComment> {
  const { data } = await http.patch<RumComment>(
    `/rum/sessions/${encodeURIComponent(sessionId)}/comments/${encodeURIComponent(commentId)}`,
    { body },
  );
  return data;
}

export async function removeComment(sessionId: string, commentId: string): Promise<void> {
  await http.delete(`/rum/sessions/${encodeURIComponent(sessionId)}/comments/${encodeURIComponent(commentId)}`);
}

/** One stretch of a session, as a model tells it. */
export interface RumSummaryChapter {
  /** When it begins, in microseconds since the epoch. */
  atTime: number;
  title: string;
  summary: string;
}

export interface RumSessionSummary {
  sessionId: string;
  language: string;
  summary: string;
  chapters: RumSummaryChapter[];
  model: string;
  promptTokens: number;
  completionTokens: number;
  generatedAt: number;
}

/** The summary kept of a session in the reader's language, if anyone asked for one. */
export async function sessionSummary(sessionId: string, signal?: AbortSignal): Promise<RumSessionSummary | null> {
  const { data } = await http.get<{ summary: RumSessionSummary | null }>(
    `/rum/sessions/${encodeURIComponent(sessionId)}/summary`,
    signal ? { signal } : {},
  );
  return data.summary;
}

/** Asks the model of the organization for it, or gives the one kept. */
export async function generateSessionSummary(
  sessionId: string,
  options: { regenerate?: boolean; range?: { from: number; to: number } | undefined } = {},
): Promise<RumSessionSummary | null> {
  const { data } = await http.post<{ summary: RumSessionSummary | null }>(
    `/rum/sessions/${encodeURIComponent(sessionId)}/summary`,
    { regenerate: options.regenerate === true },
    options.range ? { params: options.range } : {},
  );
  return data.summary;
}

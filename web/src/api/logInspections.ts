import { http } from '@/lib/http';

import type { NotifyTargetType } from './notify';

/** How often an inspection runs. Times of day are read in the inspection's time zone. */
export type InspectionSchedule =
  | { type: 'interval'; everyMinutes: number }
  | { type: 'daily'; hour: number; minute: number }
  /** `weekday`: 1 is Monday, 7 is Sunday. */
  | { type: 'weekly'; weekday: number; hour: number; minute: number };

/** Which verdicts of a report are told to the channels. */
export type InspectionNotifyWhen = 'abnormal' | 'attention' | 'always';

/** The verdict a report states; `unknown` when it states none. */
export type InspectionLevel = 'normal' | 'attention' | 'abnormal' | 'unknown';

export type InspectionRunStatus = 'queued' | 'running' | 'succeeded' | 'failed' | 'timed_out';

export type InspectionRunTrigger = 'schedule' | 'manual';

/** Why a run left no report. */
export type InspectionRunFailure =
  | 'timed_out'
  | 'interrupted'
  | 'not_started'
  | 'identity'
  | 'model'
  | 'agent'
  | 'budget'
  | 'internal';

export interface InspectionNotifyRoute {
  connectorId: string;
  targetType: NotifyTargetType;
  target: string;
}

export interface InspectionUser {
  id: string;
  displayName: string;
  email: string;
}

export interface InspectionLastRun {
  atTime: number;
  status: InspectionRunStatus;
  /** Only a run that succeeded has a verdict. */
  level: InspectionLevel | null;
}

/** The run of an inspection that waits or is under way. */
export interface InspectionActiveRun {
  id: string;
  status: InspectionRunStatus;
  trigger: InspectionRunTrigger;
  createdAt: number;
  startedAt: number | null;
}

export interface LogInspection {
  id: string;
  name: string;
  stream: string;
  /** The query as it was written on the Logs page; empty for every log of the stream. */
  statement: string;
  /** How much time before the run each run looks over. */
  windowSecs: number;
  requirements: string;
  schedule: InspectionSchedule;
  /** IANA name of the time zone the schedule and the notifications are written in. */
  timezone: string;
  providerId: string;
  /** Null: the default model of the provider at the time of each run. */
  model: string | null;
  notifyWhen: InspectionNotifyWhen;
  notifyRoutes: InspectionNotifyRoute[];
  /** The language of the report and of the notifications. */
  language: string;
  enabled: boolean;
  /** Every run reads the logs with the access this user has at that time. */
  runAsUserId: string;
  /** Null once the user record is gone. */
  runAsUser: InspectionUser | null;
  /** Null while the inspection is switched off. */
  nextRunAt: number | null;
  lastRun: InspectionLastRun | null;
  activeRun: InspectionActiveRun | null;
  createdBy: string;
  createdAt: number;
  updatedAt: number;
}

/** What a save says: the whole definition, for a new inspection and for a changed one alike. */
export interface LogInspectionInput {
  name: string;
  stream: string;
  statement: string;
  windowSecs: number;
  requirements: string;
  schedule: InspectionSchedule;
  timezone: string;
  providerId: string;
  model: string | null;
  notifyWhen: InspectionNotifyWhen;
  notifyRoutes: InspectionNotifyRoute[];
  language: string;
  enabled: boolean;
}

export interface InspectionToolCall {
  name: string;
  ok: boolean;
}

export interface InspectionNotification {
  connectorId: string;
  targetType: NotifyTargetType;
  targetMasked: string;
  delivered: boolean;
  error: string | null;
}

export interface LogInspectionRun {
  id: string;
  inspectionId: string;
  trigger: InspectionRunTrigger;
  status: InspectionRunStatus;
  level: InspectionLevel | null;
  windowStart: number;
  windowEnd: number;
  summary: string | null;
  /** What the model wrote. Left out of a list; read with the run itself. */
  report: string | null;
  failure: InspectionRunFailure | null;
  /** The technical detail of a failure. */
  error: string | null;
  providerId: string | null;
  model: string | null;
  runAsUserId: string | null;
  /** Who asked for a manual run; null for a scheduled one. */
  requestedBy: string | null;
  requestedByUser: InspectionUser | null;
  toolCalls: InspectionToolCall[];
  promptTokens: number;
  completionTokens: number;
  notifications: InspectionNotification[];
  createdAt: number;
  startedAt: number | null;
  finishedAt: number | null;
}

/** What the instance allows scheduled inspections to use. */
export interface InspectionLimits {
  minIntervalSecs: number;
  maxPerOrg: number;
  runTimeoutSecs: number;
  reportRetentionDays: number;
}

const BASE = '/logs/inspections';
const inspectionPath = (id: string) => `${BASE}/${encodeURIComponent(id)}`;

export async function list(): Promise<LogInspection[]> {
  const { data } = await http.get<LogInspection[]>(BASE);
  return data;
}

export async function limits(): Promise<InspectionLimits> {
  const { data } = await http.get<InspectionLimits>(`${BASE}/limits`);
  return data;
}

export async function get(id: string): Promise<LogInspection> {
  const { data } = await http.get<LogInspection>(inspectionPath(id));
  return data;
}

export async function create(input: LogInspectionInput): Promise<LogInspection> {
  const { data } = await http.post<LogInspection>(BASE, input);
  return data;
}

/** Saving hands the inspection to whoever saves it: it runs with their access from then on. */
export async function update(id: string, input: LogInspectionInput): Promise<LogInspection> {
  const { data } = await http.put<LogInspection>(inspectionPath(id), input);
  return data;
}

/** Pauses or resumes the schedule; what is inspected and who it runs as stay as they are. */
export async function setEnabled(id: string, enabled: boolean): Promise<LogInspection> {
  const { data } = await http.post<LogInspection>(`${inspectionPath(id)}/enabled`, { enabled });
  return data;
}

export async function remove(id: string): Promise<void> {
  await http.delete(inspectionPath(id));
}

/** Queues one run now; the backend picks it up within seconds. */
export async function runNow(id: string): Promise<LogInspectionRun> {
  const { data } = await http.post<LogInspectionRun>(`${inspectionPath(id)}/run`);
  return data;
}

/** The runs of an inspection, the most recent first, without their reports. */
export async function listRuns(
  id: string,
  params: { before?: number | undefined; limit?: number | undefined } = {},
): Promise<LogInspectionRun[]> {
  const { data } = await http.get<LogInspectionRun[]>(`${inspectionPath(id)}/runs`, {
    params: {
      limit: params.limit ?? 50,
      ...(params.before ? { before: params.before } : {}),
    },
  });
  return data;
}

/** One run with the report it left. */
export async function getRun(id: string, runId: string): Promise<LogInspectionRun> {
  const { data } = await http.get<LogInspectionRun>(
    `${inspectionPath(id)}/runs/${encodeURIComponent(runId)}`,
  );
  return data;
}

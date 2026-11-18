import { http } from '@/lib/http';

/** Where a drill's reference root cause comes from: the incident's postmortem, or written by hand. */
export type DrillSource = 'postmortem' | 'manual';

/** A run investigates, waits to be graded or is being graded, then is done or failed. */
export type DrillRunStatus = 'running' | 'grading' | 'done' | 'failed';

/** Why a run ended without a score. */
export type DrillFailure =
  | 'budget_exhausted'
  | 'no_model'
  | 'incident_gone'
  | 'start_failed'
  | 'run_failed'
  | 'run_cancelled'
  | 'run_interrupted'
  | 'no_answer'
  | 'grading_failed';

/** A resolved incident to investigate again, and the root cause the answer is compared with. */
export interface Drill {
  id: string;
  orgId: string;
  incidentId: string;
  /** The incident's summary when the drill was made. */
  title: string;
  reference: string;
  source: DrillSource;
  createdBy: string;
  createdAt: number;
  updatedAt: number;
}

export interface DrillRun {
  id: string;
  orgId: string;
  drillId: string;
  /** The hidden chat the investigation ran in. */
  chatId: string | null;
  runId: string | null;
  status: DrillRunStatus;
  /** 0 to 1 once graded. */
  score: number | null;
  /** Why the grading model gave the score, in the language of whoever started the run. */
  reasoning: string;
  reason: DrillFailure | null;
  detail: string | null;
  /** The model that investigated, and the prompt version it answered with. */
  model: string | null;
  promptVersion: number | null;
  graderModel: string | null;
  language: string;
  /** Runs started together share it. */
  batchId: string | null;
  attempts: number;
  startedBy: string;
  createdAt: number;
  finishedAt: number | null;
  updatedAt: number;
}

/** A drill with how its runs went. */
export interface DrillSummary extends Drill {
  runs: number;
  /** The mean score of its graded runs. */
  meanScore: number | null;
  lastRun: DrillRun | null;
}

/** An incident resolved in the last 180 days whose postmortem states a root cause. */
export interface DrillCandidate {
  incidentId: string;
  summary: string;
  severity: string;
  createdAt: number;
  resolvedAt: number | null;
  /** The postmortem's root cause, cut when long. */
  rootCause: string;
  /** A drill was already made from it. */
  drilled: boolean;
}

export interface NewDrill {
  incidentId: string;
  /** Written by hand; the postmortem's root cause when left out. */
  reference?: string;
}

export async function listDrills(): Promise<DrillSummary[]> {
  const { data } = await http.get<{ drills: DrillSummary[] }>('/agent/quality/drills');
  return data.drills ?? [];
}

export async function listCandidates(): Promise<DrillCandidate[]> {
  const { data } = await http.get<{ candidates: DrillCandidate[] }>('/agent/quality/drills/candidates');
  return data.candidates ?? [];
}

export async function createDrill(input: NewDrill): Promise<Drill> {
  const { data } = await http.post<Drill>('/agent/quality/drills', input);
  return data;
}

export async function deleteDrill(id: string): Promise<void> {
  await http.delete(`/agent/quality/drills/${encodeURIComponent(id)}`);
}

export async function listRuns(id: string): Promise<{ drill: Drill; runs: DrillRun[] }> {
  const { data } = await http.get<{ drill: Drill; runs: DrillRun[] }>(
    `/agent/quality/drills/${encodeURIComponent(id)}/runs`,
  );
  return data;
}

export async function runDrill(id: string): Promise<DrillRun> {
  const { data } = await http.post<{ run: DrillRun }>(
    `/agent/quality/drills/${encodeURIComponent(id)}/runs`,
  );
  return data.run;
}

/** Starts a run of every drill (or of those named) that has none going. */
export async function runAll(drillIds?: string[]): Promise<{ batchId: string; runs: DrillRun[]; skipped: number }> {
  const { data } = await http.post<{ batchId: string; runs: DrillRun[]; skipped: number }>(
    '/agent/quality/drills/runs',
    drillIds ? { drillIds } : {},
  );
  return data;
}

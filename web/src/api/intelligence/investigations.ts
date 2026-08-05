import { http } from '@/lib/http';

export type ConfidenceLevel = 'high' | 'medium' | 'low';
export type InvestigationStatus =
  | 'draft'
  | 'pending'
  | 'running'
  | 'waiting_for_data'
  | 'waiting_for_approval'
  | 'verifying_recovery'
  | 'completed'
  | 'partially_completed'
  | 'failed'
  | 'cancelled';
export type StepStatus = 'pending' | 'running' | 'succeeded' | 'failed' | 'skipped' | 'cancelled';
export type FactStatus = 'verified' | 'inference' | 'suggestion' | 'unverified';
export type HypothesisStatus =
  | 'proposed'
  | 'testing'
  | 'supported'
  | 'insufficient_evidence'
  | 'rejected';

export interface IntelligenceOverview {
  active_investigations: number;
  pending_approvals: number;
  recent_completed: number;
  automation_runs: number;
  enabled_automations: number;
}

export interface Investigation {
  id: string;
  org_id: string;
  created_by: string;
  chat_id?: string | null;
  title: string;
  status: InvestigationStatus;
  context: Record<string, unknown>;
  summary?: string | null;
  confidence?: ConfidenceLevel | null;
  current_step?: string | null;
  started_at?: number | null;
  completed_at?: number | null;
  created_at: number;
  updated_at: number;
}

export interface InvestigationStep {
  id: string;
  investigation_id: string;
  position: number;
  title: string;
  status: StepStatus;
  tool_name?: string | null;
  input: Record<string, unknown>;
  output_summary?: string | null;
  conclusion_impact?: string | null;
  error?: string | null;
  started_at?: number | null;
  ended_at?: number | null;
  created_at: number;
}

export interface InvestigationEvidence {
  id: string;
  investigation_id: string;
  step_id?: string | null;
  kind: string;
  label: string;
  fact_status: FactStatus;
  source_ref: Record<string, unknown>;
  query?: string | null;
  parameters: Record<string, unknown>;
  summary: string;
  created_at: number;
}

export interface InvestigationHypothesis {
  id: string;
  investigation_id: string;
  statement: string;
  confidence: ConfidenceLevel;
  status: HypothesisStatus;
  evidence_ids: string[];
  created_at: number;
  updated_at: number;
}

export interface InvestigationDetail {
  investigation: Investigation;
  steps: InvestigationStep[];
  evidence: InvestigationEvidence[];
  hypotheses: InvestigationHypothesis[];
}

export async function overview(): Promise<IntelligenceOverview> {
  const { data } = await http.get<IntelligenceOverview>('/intelligence/overview');
  return data;
}

export async function listInvestigations(): Promise<Investigation[]> {
  const { data } =
    await http.get<{ investigations: Investigation[] }>('/intelligence/investigations');
  return data.investigations ?? [];
}

export async function createInvestigation(input: {
  title: string;
  chat_id?: string;
  context?: Record<string, unknown>;
  steps?: string[];
}): Promise<Investigation> {
  const { data } = await http.post<Investigation>('/intelligence/investigations', input);
  return data;
}

export async function getInvestigation(id: string): Promise<InvestigationDetail> {
  const { data } = await http.get<InvestigationDetail>(
    `/intelligence/investigations/${encodeURIComponent(id)}`,
  );
  return data;
}

export async function updateInvestigation(
  id: string,
  input: Partial<
    Pick<
      Investigation,
      'title' | 'status' | 'summary' | 'confidence' | 'current_step' | 'context'
    >
  >,
): Promise<Investigation> {
  const { data } = await http.put<Investigation>(
    `/intelligence/investigations/${encodeURIComponent(id)}`,
    input,
  );
  return data;
}

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

export interface AgentOverview {
  activeInvestigations: number;
  pendingApprovals: number;
  recentCompleted: number;
  automationRuns: number;
  enabledAutomations: number;
}

export interface Investigation {
  id: string;
  orgId: string;
  createdBy: string;
  chatId?: string | null;
  title: string;
  status: InvestigationStatus;
  context: Record<string, unknown>;
  summary?: string | null;
  confidence?: ConfidenceLevel | null;
  currentStep?: string | null;
  startedAt?: number | null;
  completedAt?: number | null;
  createdAt: number;
  updatedAt: number;
}

export interface InvestigationStep {
  id: string;
  investigationId: string;
  position: number;
  title: string;
  status: StepStatus;
  toolName?: string | null;
  input: Record<string, unknown>;
  outputSummary?: string | null;
  conclusionImpact?: string | null;
  error?: string | null;
  startedAt?: number | null;
  endedAt?: number | null;
  createdAt: number;
}

export interface InvestigationEvidence {
  id: string;
  investigationId: string;
  stepId?: string | null;
  kind: string;
  label: string;
  factStatus: FactStatus;
  sourceRef: Record<string, unknown>;
  query?: string | null;
  parameters: Record<string, unknown>;
  summary: string;
  createdAt: number;
}

export interface InvestigationHypothesis {
  id: string;
  investigationId: string;
  statement: string;
  confidence: ConfidenceLevel;
  status: HypothesisStatus;
  evidenceIds: string[];
  createdAt: number;
  updatedAt: number;
}

export interface InvestigationDetail {
  investigation: Investigation;
  steps: InvestigationStep[];
  evidence: InvestigationEvidence[];
  hypotheses: InvestigationHypothesis[];
}

export async function overview(): Promise<AgentOverview> {
  const { data } = await http.get<AgentOverview>('/agent/overview');
  return data;
}

export async function listInvestigations(): Promise<Investigation[]> {
  const { data } =
    await http.get<{ investigations: Investigation[] }>('/agent/investigations');
  return data.investigations ?? [];
}

export async function createInvestigation(input: {
  title: string;
  chatId?: string;
  context?: Record<string, unknown>;
  steps?: string[];
}): Promise<Investigation> {
  const { data } = await http.post<Investigation>('/agent/investigations', input);
  return data;
}

export async function getInvestigation(id: string): Promise<InvestigationDetail> {
  const { data } = await http.get<InvestigationDetail>(
    `/agent/investigations/${encodeURIComponent(id)}`,
  );
  return data;
}

export async function updateInvestigation(
  id: string,
  input: Partial<
    Pick<
      Investigation,
      'title' | 'status' | 'summary' | 'confidence' | 'currentStep' | 'context'
    >
  >,
): Promise<Investigation> {
  const { data } = await http.put<Investigation>(
    `/agent/investigations/${encodeURIComponent(id)}`,
    input,
  );
  return data;
}

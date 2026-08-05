import { http } from '@/lib/http';

export type RiskLevel = 'l0' | 'l1' | 'l2' | 'l3' | 'l4';
export type ApprovalStatus =
  | 'pending'
  | 'approved'
  | 'rejected'
  | 'expired'
  | 'cancelled'
  | 'executed';
export type ExecutionStatus =
  | 'pending'
  | 'running'
  | 'succeeded'
  | 'partially_succeeded'
  | 'failed'
  | 'cancelled'
  | 'rolled_back'
  | 'verification_failed';

export interface Automation {
  id: string;
  name: string;
  description: string;
  enabled: boolean;
  trigger: Record<string, unknown>;
  input_context: Record<string, unknown>;
  steps: unknown;
  allowed_tools: string[];
  approval_policy: Record<string, unknown>;
  output_actions: unknown;
  failure_policy: Record<string, unknown>;
  notification: Record<string, unknown>;
  created_by: string;
  created_at: number;
  updated_at: number;
}

export type AutomationInput = Omit<
  Automation,
  'id' | 'created_by' | 'created_at' | 'updated_at'
>;

export interface ApprovalRequest {
  id: string;
  investigation_id?: string | null;
  action: string;
  target: string;
  parameters: Record<string, unknown>;
  reason: string;
  impact: string;
  risk: RiskLevel;
  status: ApprovalStatus;
  requested_by: string;
  required_approvals: number;
  reviews: Array<Record<string, unknown>>;
  expires_at?: number | null;
  decided_at?: number | null;
  created_at: number;
  updated_at: number;
}

export interface Execution {
  id: string;
  approval_request_id: string;
  investigation_id?: string | null;
  action: string;
  target: string;
  parameters: Record<string, unknown>;
  idempotency_key: string;
  requested_by: string;
  approved_by: string[];
  status: ExecutionStatus;
  output_summary?: string | null;
  error?: string | null;
  verification: Record<string, unknown>;
  started_at?: number | null;
  finished_at?: number | null;
  created_at: number;
  updated_at: number;
}

export async function listAutomations(): Promise<Automation[]> {
  const { data } = await http.get<{ automations: Automation[] }>('/intelligence/automations');
  return data.automations ?? [];
}

export async function createAutomation(input: AutomationInput): Promise<Automation> {
  const { data } = await http.post<Automation>('/intelligence/automations', input);
  return data;
}

export async function updateAutomation(
  id: string,
  input: AutomationInput,
): Promise<Automation> {
  const { data } = await http.put<Automation>(
    `/intelligence/automations/${encodeURIComponent(id)}`,
    input,
  );
  return data;
}

export async function dryRunAutomation(
  id: string,
  event: Record<string, unknown>,
): Promise<Record<string, unknown>> {
  const { data } = await http.post<Record<string, unknown>>(
    `/intelligence/automations/${encodeURIComponent(id)}/dry-run`,
    event,
  );
  return data;
}

export async function listApprovals(): Promise<ApprovalRequest[]> {
  const { data } = await http.get<{ approvals: ApprovalRequest[] }>('/intelligence/approvals');
  return data.approvals ?? [];
}

export async function reviewApproval(
  id: string,
  approve: boolean,
  comment: string,
): Promise<ApprovalRequest> {
  const { data } = await http.post<ApprovalRequest>(
    `/intelligence/approvals/${encodeURIComponent(id)}/review`,
    { approve, comment },
  );
  return data;
}

export async function executeApproval(id: string, idempotencyKey: string): Promise<Execution> {
  const { data } = await http.post<Execution>(
    `/intelligence/approvals/${encodeURIComponent(id)}/execute`,
    { idempotency_key: idempotencyKey },
  );
  return data;
}

export async function listExecutions(): Promise<Execution[]> {
  const { data } = await http.get<{ executions: Execution[] }>('/intelligence/executions');
  return data.executions ?? [];
}

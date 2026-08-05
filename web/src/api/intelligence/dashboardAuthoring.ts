import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

import type { DashboardDefinition } from '@/dashboard-engine/schema';
import { http } from '@/lib/http';

import {
  executeApproval,
  type ApprovalRequest,
  type Execution,
} from './automations';

export type DashboardDraftStatus = 'ready' | 'consumed' | 'expired';

export interface DashboardContractIssue {
  code: string;
  path: string;
  message: string;
}

export interface DashboardPreflightWarning {
  code: string;
  path: string;
  message: string;
}

export interface DashboardPanelPreflight {
  path: string;
  title: string;
  query_kind: string;
  status: 'passed' | 'empty' | 'skipped';
  tested_from_micros: number;
  tested_to_micros: number;
  returned_rows: number;
  scanned_rows: number;
  took_ms: number;
}

export interface DashboardPreflightReport {
  panels: DashboardPanelPreflight[];
  warnings: DashboardPreflightWarning[];
  issues: DashboardContractIssue[];
}

export interface DashboardDraftOperation {
  approval_id: string;
  status: ApprovalRequest['status'];
  required_approvals: number;
  approved_reviews: number;
  expires_at?: number | null;
}

export interface DashboardDraftPreview {
  draft_id: string;
  model_hash: string;
  folder_id?: string | null;
  status: DashboardDraftStatus;
  created_at: number;
  expires_at: number;
  compiled_model: DashboardDefinition;
  warnings: DashboardPreflightWarning[];
  preflight: DashboardPreflightReport;
  operation?: DashboardDraftOperation | null;
  dashboard_id?: string | null;
  dashboard_route?: string | null;
  can_propose: boolean;
}

export interface DashboardAuthoringCapabilities {
  authoring_versions: number[];
  dashboard_model_version: number;
  compiler_version: string;
  query_kinds: string[];
  visualizations: Array<Record<string, unknown>>;
  units: string[];
  reducers: string[];
  limits: Record<string, unknown>;
  workflow: string[];
}

export interface ProposeDashboardInput {
  draftId: string;
  expectedHash: string;
  reason: string;
  impact: string;
}

export async function getDashboardAuthoringCapabilities(): Promise<DashboardAuthoringCapabilities> {
  const { data } = await http.get<DashboardAuthoringCapabilities>(
    '/intelligence/dashboard-authoring/capabilities',
  );
  return data;
}

export async function getDashboardDraft(draftId: string): Promise<DashboardDraftPreview> {
  const { data } = await http.get<DashboardDraftPreview>(
    `/intelligence/dashboard-drafts/${encodeURIComponent(draftId)}`,
  );
  return data;
}

export async function proposeDashboardCreation(
  input: ProposeDashboardInput,
): Promise<ApprovalRequest> {
  const { data } = await http.post<{ approval: ApprovalRequest }>(
    `/intelligence/dashboard-drafts/${encodeURIComponent(input.draftId)}/propose`,
    {
      expected_hash: input.expectedHash,
      reason: input.reason,
      impact: input.impact,
    },
  );
  return data.approval;
}

export function useDashboardDraft(draftId: string) {
  return useQuery({
    queryKey: ['intelligence', 'dashboard-draft', draftId],
    queryFn: () => getDashboardDraft(draftId),
    enabled: Boolean(draftId),
    retry: false,
  });
}

export function useProposeDashboardCreation() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: proposeDashboardCreation,
    onSuccess: (_approval, input) =>
      queryClient.invalidateQueries({
        queryKey: ['intelligence', 'dashboard-draft', input.draftId],
      }),
  });
}

export interface DashboardExecutionResult extends Execution {
  verification: Execution['verification'] & {
    dashboard_id?: string;
    dashboard_route?: string;
    draft_consumed?: boolean;
    replayed?: boolean;
  };
}

export function useExecuteDashboardCreation(draftId: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({
      approvalId,
      idempotencyKey,
    }: {
      approvalId: string;
      idempotencyKey: string;
    }) =>
      executeApproval(
        approvalId,
        idempotencyKey,
      ) as Promise<DashboardExecutionResult>,
    onSuccess: () =>
      queryClient.invalidateQueries({
        queryKey: ['intelligence', 'dashboard-draft', draftId],
      }),
  });
}

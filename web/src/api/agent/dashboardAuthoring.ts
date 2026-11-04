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
  queryKind: string;
  status: 'passed' | 'empty' | 'skipped';
  testedFrom: number;
  testedTo: number;
  returnedRows: number;
  scannedRows: number;
  tookMs: number;
}

export interface DashboardPreflightReport {
  panels: DashboardPanelPreflight[];
  warnings: DashboardPreflightWarning[];
  issues: DashboardContractIssue[];
}

export interface DashboardDraftOperation {
  approvalId: string;
  status: ApprovalRequest['status'];
  requiredApprovals: number;
  approvedReviews: number;
  expiresAt?: number | null;
}

export interface DashboardDraftPreview {
  draftId: string;
  modelHash: string;
  folderId?: string | null;
  status: DashboardDraftStatus;
  createdAt: number;
  expiresAt: number;
  compiledModel: DashboardDefinition;
  warnings: DashboardPreflightWarning[];
  preflight: DashboardPreflightReport;
  operation?: DashboardDraftOperation | null;
  dashboardId?: string | null;
  dashboardRoute?: string | null;
  canPropose: boolean;
}

export interface DashboardAuthoringCapabilities {
  authoringVersions: number[];
  dashboardModelVersion: number;
  compilerVersion: string;
  queryKinds: string[];
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
    '/agent/dashboard-authoring/capabilities',
  );
  return data;
}

export async function getDashboardDraft(draftId: string): Promise<DashboardDraftPreview> {
  const { data } = await http.get<DashboardDraftPreview>(
    `/agent/dashboard-drafts/${encodeURIComponent(draftId)}`,
  );
  return data;
}

export async function proposeDashboardCreation(
  input: ProposeDashboardInput,
): Promise<ApprovalRequest> {
  const { data } = await http.post<{ approval: ApprovalRequest }>(
    `/agent/dashboard-drafts/${encodeURIComponent(input.draftId)}/propose`,
    {
      expectedHash: input.expectedHash,
      reason: input.reason,
      impact: input.impact,
    },
  );
  return data.approval;
}

export function useDashboardDraft(draftId: string) {
  return useQuery({
    queryKey: ['agent', 'dashboard-draft', draftId],
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
        queryKey: ['agent', 'dashboard-draft', input.draftId],
      }),
  });
}

export interface DashboardExecutionResult extends Execution {
  verification: Execution['verification'] & {
    dashboardId?: string;
    dashboardRoute?: string;
    draftConsumed?: boolean;
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
        queryKey: ['agent', 'dashboard-draft', draftId],
      }),
  });
}

import { useQuery } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';

import { listApprovals } from '@/api/agent/automations';
import * as incidentsApi from '@/api/incidents';
import * as intakeUsageApi from '@/api/intakeUsage';
import * as pipelinesApi from '@/api/pipelines';
import * as statusPagesApi from '@/api/statusPages';
import * as usersApi from '@/api/users';
import { canAccessProductPath, hasPermission, useProductAccess } from '@/product/access';
import { courseDuration } from '@/shell/incident/Course';
import { useAuthStore } from '@/stores/auth';

import { byUrgency, isWaitingOn, rowsOf, type AppNotification } from './model';

/**
 * Every source of the bell. A new kind of notification is one more hook here,
 * merged at the end of `useAppNotifications`: nothing in the bell itself changes.
 */

function useIncidentNotifications(): AppNotification[] {
  const { t } = useTranslation('shell');
  const ctx = useAuthStore((state) => state.ctx);
  const access = useProductAccess();

  const query = useQuery({
    queryKey: ['alerts', 'incidents', 'active'],
    queryFn: () => incidentsApi.list(),
    enabled: !!ctx && canAccessProductPath('/alerts/incidents', access),
    refetchInterval: 30_000,
  });

  const nowMicros = Date.now() * 1000;

  return rowsOf(query.data)
    .filter((incident) => isWaitingOn(incident, ctx?.userId ?? ''))
    .sort(byUrgency)
    .map((incident) => ({
      id: `incident:${incident.id}`,
      category: 'alerts',
      title: incident.summary || incident.id,
      description: t('notifications.incident_meta', {
        age: courseDuration(nowMicros - incident.createdAt),
        service: incident.affectedServices[0] ?? t('notifications.no_service'),
      }),
      severity: incident.severity,
      incidentId: incident.id,
      to: `/alerts/incidents/${encodeURIComponent(incident.id)}`,
    }));
}

function usePipelineNotifications(): AppNotification[] {
  const { t } = useTranslation('shell');
  const ctx = useAuthStore((state) => state.ctx);
  const access = useProductAccess();

  const query = useQuery({
    queryKey: ['pipelines', 'list'],
    queryFn: () => pipelinesApi.list(),
    enabled: !!ctx && canAccessProductPath('/pipelines', access),
    refetchInterval: 60_000,
  });

  return rowsOf(query.data)
    .filter((pipeline) => pipeline.lastRunState === 'failed')
    .map((pipeline) => ({
      // Failing again later is news again: the run's time is part of what was read.
      id: `pipeline:${pipeline.id}:${pipeline.lastRunAt ?? 0}`,
      category: 'alerts',
      title: t('notifications.pipeline_failed', { name: pipeline.name }),
      to: `/pipelines/${encodeURIComponent(pipeline.id)}`,
    }));
}

/** 待审批的自助注册用户（仅管理员可见）。 */
function useMemberApprovalNotifications(): AppNotification[] {
  const { t } = useTranslation('shell');
  const ctx = useAuthStore((state) => state.ctx);
  const access = useProductAccess();
  const canManageMembers = hasPermission('org.members.approve', access);

  const query = useQuery({
    queryKey: ['iam', 'users'],
    queryFn: () => usersApi.list(),
    enabled: canManageMembers && !!ctx,
    refetchInterval: 60_000,
  });

  return rowsOf(query.data)
    .filter((user) => user.status === 'pending')
    .map((user) => ({
      id: `approval:${user.id}`,
      category: 'approvals',
      title: t('notifications.pending_user'),
      description: user.displayName || user.email,
      to: '/iam/approvals',
    }));
}

function useAgentApprovalNotifications(): AppNotification[] {
  const { t } = useTranslation('shell');
  const ctx = useAuthStore((state) => state.ctx);
  const access = useProductAccess();

  const query = useQuery({
    queryKey: ['agent', 'approvals'],
    queryFn: listApprovals,
    enabled: !!ctx && hasPermission('agent.approve', access),
    refetchInterval: 60_000,
  });

  return rowsOf(query.data)
    .filter((approval) => approval.status === 'pending')
    .map((approval) => ({
      id: `agent-approval:${approval.id}`,
      category: 'approvals',
      title: t('notifications.agent_approval', { action: approval.action }),
      description: approval.target,
      to: '/agent/approvals?status=pending',
    }));
}

function useStatusPageAutomationNotifications(): AppNotification[] {
  const { t } = useTranslation('status-pages');
  const ctx = useAuthStore((state) => state.ctx);
  const access = useProductAccess();
  const canPublish = hasPermission('status_pages.automation.approve', access);

  const query = useQuery({
    queryKey: ['status-pages', 'automation', 'pending'],
    queryFn: statusPagesApi.listPendingAutomationCandidates,
    enabled: canPublish && !!ctx,
    refetchInterval: 30_000,
  });

  return rowsOf(query.data).map((candidate) => ({
    id: `status-page-automation:${candidate.id}`,
    category: 'approvals',
    title: t(`automation.notification.${candidate.state}`),
    description: candidate.title,
    to: `/status-pages/${encodeURIComponent(candidate.statusPageId)}/automation/${encodeURIComponent(candidate.id)}`,
  }));
}

/** A source past or near its budget. Raising the limit or a new period makes it news again. */
function useIntakeBudgetNotifications(): AppNotification[] {
  const { t } = useTranslation('shell');
  const ctx = useAuthStore((state) => state.ctx);
  const access = useProductAccess();

  const query = useQuery({
    queryKey: ['intake-usage', 'budgets'],
    queryFn: intakeUsageApi.listBudgets,
    enabled: !!ctx && access?.scope !== 'system' && canAccessProductPath('/streams/usage', access),
    refetchInterval: 60_000,
  });

  return rowsOf(query.data).flatMap((budget) => {
    if (budget.state !== 'warning' && budget.state !== 'exceeded') return [];

    return [
      {
        id: `intake-budget:${budget.id}:${budget.periodStart}:${budget.limitSize}:${budget.state}`,
        category: 'alerts' as const,
        title: t(`notifications.intake_budget_${budget.state}`, { name: budget.label.name }),
        description: t('notifications.intake_budget_meta', {
          percent: Math.floor((budget.usedRatio ?? 0) * 100),
        }),
        severity: budget.state === 'exceeded' ? ('error' as const) : ('warning' as const),
        to: `/streams/usage${budget.dimension === 'credential' ? '' : `?dimension=${budget.dimension}`}`,
      },
    ];
  });
}

/** 聚合所有来源的通知：要紧的在前。 */
export function useAppNotifications(): AppNotification[] {
  const incidents = useIncidentNotifications();
  const pipelines = usePipelineNotifications();
  const memberApprovals = useMemberApprovalNotifications();
  const agentApprovals = useAgentApprovalNotifications();
  const statusPageAutomation = useStatusPageAutomationNotifications();
  const intakeBudgets = useIntakeBudgetNotifications();

  return [
    ...incidents,
    ...pipelines,
    ...intakeBudgets,
    ...agentApprovals,
    ...memberApprovals,
    ...statusPageAutomation,
  ];
}

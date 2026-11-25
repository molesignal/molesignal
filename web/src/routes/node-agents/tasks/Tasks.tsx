import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import type { TFunction } from 'i18next';
import { Gauge, PlaySquare } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useParams, useSearchParams } from 'react-router-dom';

import { DataTable, type DataTableColumn } from '@/admin';
import * as nodeAgentsApi from '@/api/nodeAgents';
import {
  type NodeTask,
  type NodeTaskQuota,
  NODE_TASK_STATES,
  type NodeTaskState,
  nodeAgentKeys,
} from '@/api/nodeAgents';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { FittedSelect } from '@/shell/ui/fitted-select';
import { toast } from '@/shell/ui/sonner';
import { formatRelativeMicros } from '@/time/relative';

import { NODE_AGENTS_REFRESH_MILLIS, NodeAgentsPage, TaskStateMark, useLoadState } from '../Frame';
import { nodeName, taskSubject } from '../model';
import { QUOTA_FIELDS, QuotaDrawer } from './QuotaDrawer';
import { RunTaskDrawer } from './RunTaskDrawer';
import { TaskDrawer } from './TaskDrawer';

const ALL = 'all';

/**
 * The diagnostic tasks sent to nodes, the latest first: what each asked, where it stands, and what
 * came back. The quota they count against sits under the list.
 */
export function Tasks() {
  const { t, i18n } = useTranslation('infrastructure');
  const navigate = useNavigate();
  const { taskId } = useParams();
  const [params, setParams] = useSearchParams();
  const queryClient = useQueryClient();
  const runAccess = useActionAccess({ permission: 'node_agents.tasks.run' });
  const manage = useActionAccess({ permission: 'node_agents.manage' });
  const [running, setRunning] = React.useState(false);
  const [quotaDraft, setQuotaDraft] = React.useState<NodeTaskQuota>();

  const nodeFilter = params.get('nodeId') ?? undefined;
  const stateParam = params.get('state') as NodeTaskState | null;
  const stateFilter = stateParam && NODE_TASK_STATES.includes(stateParam) ? stateParam : undefined;
  const filter = { nodeId: nodeFilter, states: stateFilter ? [stateFilter] : undefined, limit: 200 };

  const tasksQuery = useQuery({
    queryKey: nodeAgentKeys.tasks(filter),
    queryFn: () => nodeAgentsApi.listTasks(filter),
    refetchInterval: NODE_AGENTS_REFRESH_MILLIS,
  });

  const nodesQuery = useQuery({ queryKey: nodeAgentKeys.nodes(), queryFn: nodeAgentsApi.listNodes });
  const quotaQuery = useQuery({ queryKey: nodeAgentKeys.quota(), queryFn: nodeAgentsApi.getTaskQuota });
  const tasks = React.useMemo(() => tasksQuery.data ?? [], [tasksQuery.data]);
  const nodes = React.useMemo(() => nodesQuery.data ?? [], [nodesQuery.data]);
  const names = React.useMemo(() => new Map(nodes.map((node) => [node.id, nodeName(node)])), [nodes]);
  const runnable = React.useMemo(
    () => nodes.filter((node) => node.status !== 'revoked' && !node.drainRequestedAt),
    [nodes],
  );

  const selected = tasks.find((task) => task.id === taskId);
  const refresh = () => queryClient.invalidateQueries({ queryKey: nodeAgentKeys.all });

  const cancel = useMutation({
    mutationFn: (task: NodeTask) => nodeAgentsApi.cancelTask(task.id),
    onSuccess: async (task) => {
      toast.success(t(task.state === 'cancelled' ? 'node_agents.cancel.done' : 'node_agents.cancel.requested'));
      await refresh();
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const narrow = (key: 'nodeId' | 'state', value: string) => {
    const next = new URLSearchParams(params);
    if (!value || value === ALL) next.delete(key);
    else next.set(key, value);
    setParams(next, { replace: true });
  };

  const state = useLoadState({
    pending: tasksQuery.isPending,
    error: tasksQuery.error,
    onRetry: () => void refresh(),
  });

  const runAction = (
    <ChromeButton
      variant="primary"
      disabled={runAccess.disabled || runnable.length === 0}
      disabledReason={runAccess.reason ?? t('node_agents.run.no_nodes')}
      onClick={() => setRunning(true)}
    >
      <PlaySquare aria-hidden className="h-3.5 w-3.5" />
      {t('node_agents.actions.run_task')}
    </ChromeButton>
  );

  return (
    <>
      <NodeAgentsPage
        state={state}
        action={runAction}
        listHead={
          <>
            <FittedSelect
              aria-label={t('node_agents.task.node')}
              value={nodeFilter ?? ALL}
              maxWidth={240}
              onValueChange={(value) => narrow('nodeId', value)}
              options={[
                { value: ALL, label: t('node_agents.all_nodes') },
                ...nodes.map((node) => ({ value: node.id, label: nodeName(node) })),
              ]}
            />
            <FittedSelect
              aria-label={t('node_agents.columns.status')}
              value={stateFilter ?? ALL}
              maxWidth={180}
              onValueChange={(value) => narrow('state', value)}
              options={[
                { value: ALL, label: t('node_agents.all_states') },
                ...NODE_TASK_STATES.map((value) => ({ value, label: t(`node_agents.task_state.${value}`) })),
              ]}
            />
          </>
        }
      >
        <div className="overflow-x-auto">
          <DataTable
            rows={tasks}
            columns={taskColumns(t, i18n.language, names)}
            rowKey={(task) => task.id}
            onRowClick={(task) => navigate({ pathname: `/node-agents/tasks/${task.id}`, search: params.toString() })}
            emptyLabel={t('node_agents.tasks_empty')}
            className="min-w-[900px] rounded-none border-0 bg-transparent"
          />
        </div>
        {quotaQuery.data && (
          <div
            data-testid="node-task-quota"
            className="flex flex-wrap items-center gap-x-6 gap-y-2 border-t border-bd-0 px-3 py-2.5 text-xs text-tx-2"
          >
            <span className="inline-flex items-center gap-1.5 text-tx-1">
              <Gauge aria-hidden className="h-3.5 w-3.5 text-tx-3" />
              {t('node_agents.quota.title')}
            </span>
            {QUOTA_FIELDS.map((field) => (
              <span key={field}>
                {t(`node_agents.quota.${field}`)}: <span className="text-tx-1">{quotaQuery.data[field]}</span>
              </span>
            ))}
            {manage.allowed && (
              <ChromeButton size="sm" className="ml-auto" onClick={() => setQuotaDraft(quotaQuery.data)}>
                {t('node_agents.quota.edit')}
              </ChromeButton>
            )}
          </div>
        )}
      </NodeAgentsPage>

      <TaskDrawer
        task={selected}
        nodeName={selected ? names.get(selected.nodeId) : undefined}
        canCancel={runAccess.allowed}
        cancelling={cancel.isPending}
        onCancel={(task) => cancel.mutate(task)}
        onClose={() => navigate({ pathname: '/node-agents/tasks', search: params.toString() }, { replace: true })}
      />
      <RunTaskDrawer
        open={running}
        nodes={runnable}
        initialNodeId={nodeFilter}
        onClose={() => setRunning(false)}
        onCreated={(task) => {
          setRunning(false);
          void refresh();
          navigate(`/node-agents/tasks/${task.id}`);
        }}
      />
      <QuotaDrawer
        quota={quotaDraft}
        onClose={() => setQuotaDraft(undefined)}
        onSaved={(quota) => {
          queryClient.setQueryData(nodeAgentKeys.quota(), quota);
          setQuotaDraft(undefined);
        }}
      />
    </>
  );
}

function taskColumns(
  t: TFunction<'infrastructure'>,
  locale: string,
  names: ReadonlyMap<string, string>,
): DataTableColumn<NodeTask>[] {
  return [
    {
      key: 'task',
      header: t('node_agents.columns.task'),
      width: 200,
      cell: (task) => (
        <div className="min-w-0">
          <div className="truncate text-tx-0">{taskSubject(task)}</div>
          <div className="mt-0.5 truncate text-type-micro text-tx-3">{t(`node_agents.kind.${task.kind}`)}</div>
        </div>
      ),
    },
    {
      key: 'node',
      header: t('node_agents.task.node'),
      width: 200,
      cell: (task) => <span className="truncate text-xs text-tx-2">{names.get(task.nodeId) ?? task.nodeId}</span>,
    },
    {
      key: 'state',
      header: t('node_agents.columns.status'),
      width: 140,
      cell: (task) => <TaskStateMark state={task.state} />,
    },
    {
      key: 'attempt',
      header: t('node_agents.task.attempt'),
      width: 90,
      cell: (task) => <span className="text-xs text-tx-2">{task.attempt}/{task.maxAttempts}</span>,
    },
    {
      key: 'created',
      header: t('node_agents.task.created'),
      width: 130,
      cell: (task) => <span className="text-xs text-tx-2">{formatRelativeMicros(task.createdAt, locale)}</span>,
    },
    {
      key: 'summary',
      header: t('node_agents.task.summary'),
      cell: (task) => (
        <span className="truncate font-code text-xs text-tx-3">{task.result?.summary ?? task.failure ?? ''}</span>
      ),
    },
  ];
}


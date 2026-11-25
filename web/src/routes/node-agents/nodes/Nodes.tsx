import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import type { TFunction } from 'i18next';
import { Plus, Search } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useParams, useSearchParams } from 'react-router-dom';

import { DataTable, type DataTableColumn } from '@/admin';
import * as nodeAgentsApi from '@/api/nodeAgents';
import { type NodeAgent, type NodeStatus, nodeAgentKeys } from '@/api/nodeAgents';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ProductState } from '@/product/states';
import { ChromeButton } from '@/shell/chrome';
import { FormInput } from '@/shell/FormDrawer';
import { FittedSelect } from '@/shell/ui/fitted-select';
import { toast } from '@/shell/ui/sonner';
import { formatRelativeMicros } from '@/time/relative';

import { NODE_AGENTS_REFRESH_MILLIS, NodeAgentsPage, NodeStatusMark, useLoadState } from '../Frame';
import { countByStatus, filterNodes, NODE_STATUSES, nodeName, statusOrder } from '../model';
import { ConfigDrawer } from './ConfigDrawer';
import { DrainDrawer, RevokeDrawer } from './NodeActionDrawers';
import { NodeDrawer } from './NodeDrawer';
import { RunTaskDrawer } from '../tasks/RunTaskDrawer';

type Pending =
  | { action: 'drain' | 'revoke' | 'config'; node: NodeAgent }
  | { action: 'run'; node: NodeAgent }
  | undefined;

/**
 * The managed Node Agents of the organization: where each stands, what it runs, and what was last
 * heard from it. A node opens in a drawer with what can be done to it.
 */
export function Nodes() {
  const { t, i18n } = useTranslation('infrastructure');
  const navigate = useNavigate();
  const { nodeId } = useParams();
  const [params, setParams] = useSearchParams();
  const queryClient = useQueryClient();
  const manage = useActionAccess({ permission: 'node_agents.manage' });
  const revokeAccess = useActionAccess({ permission: 'node_agents.revoke' });
  const runAccess = useActionAccess({ permission: 'node_agents.tasks.run' });
  const enrollAccess = useActionAccess({ permission: 'node_agents.enroll' });
  const [pending, setPending] = React.useState<Pending>();

  const nodesQuery = useQuery({
    queryKey: nodeAgentKeys.nodes(),
    queryFn: nodeAgentsApi.listNodes,
    refetchInterval: NODE_AGENTS_REFRESH_MILLIS,
  });

  const nodes = React.useMemo(() => nodesQuery.data ?? [], [nodesQuery.data]);
  const query = params.get('query') ?? '';
  const statusParam = params.get('status') as NodeStatus | null;
  const status = statusParam && NODE_STATUSES.includes(statusParam) ? statusParam : 'all';
  const rows = React.useMemo(() => filterNodes(nodes, { query, status }), [nodes, query, status]);
  const counts = React.useMemo(() => countByStatus(nodes), [nodes]);
  const selected = nodes.find((node) => node.id === nodeId);
  const runnable = React.useMemo(
    () => nodes.filter((node) => node.status !== 'revoked' && !node.drainRequestedAt),
    [nodes],
  );

  const refresh = () => queryClient.invalidateQueries({ queryKey: nodeAgentKeys.all });

  const settle = async (updated: NodeAgent) => {
    queryClient.setQueryData<NodeAgent[]>(nodeAgentKeys.nodes(), (current) =>
      current?.map((node) => (node.id === updated.id ? updated : node)),
    );
    setPending(undefined);
    await refresh();
  };

  const resume = useMutation({
    mutationFn: (node: NodeAgent) => nodeAgentsApi.resumeNode(node.id),
    onSuccess: async (updated) => {
      toast.success(t('node_agents.resume_done'));
      await settle(updated);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const narrow = (key: 'query' | 'status', value: string) => {
    const next = new URLSearchParams(params);
    if (!value || value === 'all') next.delete(key);
    else next.set(key, value);
    setParams(next, { replace: true });
  };

  const state = useLoadState({
    pending: nodesQuery.isPending,
    error: nodesQuery.error,
    onRetry: () => void refresh(),
  });

  const enrollAction = (
    <ChromeButton
      variant="primary"
      disabled={enrollAccess.disabled}
      disabledReason={enrollAccess.reason}
      onClick={() => navigate('/node-agents/enrollment?create=1')}
    >
      <Plus aria-hidden className="h-3.5 w-3.5" />
      {t('node_agents.actions.enroll')}
    </ChromeButton>
  );

  const none = !state && nodes.length === 0;
  const closeDrawer = () => navigate({ pathname: '/node-agents/nodes', search: params.toString() }, { replace: true });

  return (
    <>
      <NodeAgentsPage
        state={state}
        action={none ? undefined : enrollAction}
        kpis={
          none
            ? undefined
            : (['online', 'offline', 'draining'] as const).map((key) => ({
                label: t(`node_agents.status.${key}`),
                value: String(counts[key]),
              }))
        }
        listHead={
          none ? undefined : (
            <>
              <label className="relative w-full min-w-0 sm:w-[300px]">
                <span className="sr-only">{t('node_agents.search_placeholder')}</span>
                <Search aria-hidden className="pointer-events-none absolute left-3 top-2.5 h-4 w-4 text-tx-3" />
                <FormInput
                  value={query}
                  onChange={(event) => narrow('query', event.target.value)}
                  placeholder={t('node_agents.search_placeholder')}
                  className="pl-9"
                />
              </label>
              <FittedSelect
                aria-label={t('node_agents.columns.status')}
                value={status}
                maxWidth={180}
                onValueChange={(value) => narrow('status', value)}
                options={[
                  { value: 'all', label: t('node_agents.all_statuses') },
                  ...NODE_STATUSES.map((value) => ({ value, label: t(`node_agents.status.${value}`) })),
                ]}
              />
            </>
          )
        }
      >
        {none ? (
          <ProductState
            variant="empty"
            title={t('node_agents.empty_title')}
            description={t('node_agents.empty_description')}
            action={enrollAction}
          />
        ) : (
          <div className="overflow-x-auto">
            <DataTable
              rows={rows}
              columns={nodeColumns(t, i18n.language)}
              rowKey={(node) => node.id}
              onRowClick={(node) =>
                navigate({ pathname: `/node-agents/nodes/${node.id}`, search: params.toString() })
              }
              emptyLabel={t('list.no_match_title')}
              defaultSort={{ key: 'status', direction: 'asc' }}
              className="min-w-[960px] rounded-none border-0 bg-transparent"
            />
          </div>
        )}
      </NodeAgentsPage>

      <NodeDrawer
        node={selected}
        onClose={closeDrawer}
        actions={{
          canManage: manage.allowed,
          canRevoke: revokeAccess.allowed,
          canRunTasks: runAccess.allowed,
          onDrain: (node) => setPending({ action: 'drain', node }),
          onResume: (node) => resume.mutate(node),
          onRevoke: (node) => setPending({ action: 'revoke', node }),
          onEditConfig: (node) => setPending({ action: 'config', node }),
          onRunTask: (node) => setPending({ action: 'run', node }),
        }}
      />
      <DrainDrawer
        node={pending?.action === 'drain' ? pending.node : undefined}
        onClose={() => setPending(undefined)}
        onDone={settle}
      />
      <RevokeDrawer
        node={pending?.action === 'revoke' ? pending.node : undefined}
        onClose={() => setPending(undefined)}
        onDone={settle}
      />
      <ConfigDrawer
        node={pending?.action === 'config' ? pending.node : undefined}
        onClose={() => setPending(undefined)}
        onDone={settle}
      />
      <RunTaskDrawer
        open={pending?.action === 'run'}
        nodes={runnable}
        initialNodeId={pending?.node.id}
        onClose={() => setPending(undefined)}
        onCreated={(task) => {
          setPending(undefined);
          void refresh();
          navigate(`/node-agents/tasks/${task.id}`);
        }}
      />
    </>
  );
}

function nodeColumns(t: TFunction<'infrastructure'>, locale: string): DataTableColumn<NodeAgent>[] {
  return [
    {
      key: 'name',
      header: t('node_agents.columns.name'),
      width: 240,
      value: nodeName,
      sortable: true,
      cell: (node) => (
        <div className="min-w-0">
          <div className="truncate text-tx-0">{nodeName(node)}</div>
          <div className="mt-0.5 truncate font-code text-type-micro text-tx-3">{node.hostId}</div>
        </div>
      ),
    },
    {
      key: 'status',
      header: t('node_agents.columns.status'),
      width: 120,
      value: (node) => t(`node_agents.status.${node.status}`),
      sortValue: (node) => statusOrder(node.status),
      sortable: true,
      cell: (node) => <NodeStatusMark status={node.status} />,
    },
    {
      key: 'version',
      header: t('node_agents.columns.version'),
      width: 110,
      value: (node) => node.agentVersion,
      sortable: true,
      cell: (node) => <span className="font-code text-xs text-tx-2">v{node.agentVersion}</span>,
    },
    {
      key: 'kinds',
      header: t('node_agents.columns.task_kinds'),
      width: 200,
      value: (node) => node.capabilities.taskKinds.join(', '),
      cell: (node) => (
        <span className="truncate text-xs text-tx-2">
          {node.capabilities.taskKinds.map((kind) => t(`node_agents.kind.${kind}`)).join(' · ') ||
            t('node_agents.none')}
        </span>
      ),
    },
    {
      key: 'config',
      header: t('node_agents.columns.config'),
      width: 110,
      value: (node) => node.appliedConfigRevision,
      cell: (node) => {
        const desired = node.desiredConfig?.revision ?? 0;
        return (
          <span className={desired > node.appliedConfigRevision ? 'text-xs text-yellow' : 'text-xs text-tx-2'}>
            r{node.appliedConfigRevision}
            {desired > node.appliedConfigRevision && ` → r${desired}`}
          </span>
        );
      },
    },
    {
      key: 'heartbeat',
      header: t('node_agents.columns.heartbeat'),
      width: 130,
      value: (node) => node.lastHeartbeatAt ?? 0,
      sortable: true,
      cell: (node) => (
        <span className="text-xs text-tx-2">
          {node.lastHeartbeatAt ? formatRelativeMicros(node.lastHeartbeatAt, locale) : t('node_agents.never')}
        </span>
      ),
    },
  ];
}

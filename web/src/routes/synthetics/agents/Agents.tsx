import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import type { TFunction } from 'i18next';
import { Cpu, Plus, RefreshCw, Server } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useParams, useSearchParams } from 'react-router-dom';

import { ConfirmDialog, DataTable, type DataTableColumn } from '@/admin';
import * as syntheticsApi from '@/api/synthetics';
import type { ProbeAgent } from '@/api/synthetics';
import { toApiError } from '@/lib/http';
import { hasPermission, useProductAccess } from '@/product/access';
import { ProductState } from '@/product/states';
import { ChromeButton } from '@/shell/chrome';
import { FormInput, FormSelect } from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';

import {
  SyntheticsCanvas,
  SyntheticsFilterBar,
  SyntheticsKpiBand,
  SyntheticsListSurface,
  SyntheticsPage,
  WorkspaceBoundary,
} from '../components';
import { formatRelativeTimestamp } from '../model';
import { AgentConfigurationDrawer } from './AgentConfigurationDrawer';
import { AgentDetailDrawer } from './AgentDetailDrawer';
import { AgentStatusPill, CompactCapacity } from './components';
import {
  AGENT_STATUSES,
  agentDisplayName,
  filterAgentRows,
  joinAgentLocations,
  summarizeAgents,
  type AgentRow,
  type AgentStatus,
} from './model';
import {
  AgentRegisterDrawer,
  RegisterInstructionsDrawer,
} from './RegisterDrawers';

export function Agents() {
  const { t, i18n } = useTranslation('synthetics');
  const navigate = useNavigate();
  const { agentId } = useParams();
  const [searchParams, setSearchParams] = useSearchParams();
  const queryClient = useQueryClient();
  const access = useProductAccess();
  const canManage = hasPermission('synthetics.locations.manage', access);
  const [registerOpen, setRegisterOpen] = React.useState(false);
  const [instructions, setInstructions] = React.useState<
    Awaited<ReturnType<typeof syntheticsApi.createRegisterToken>>
  >();
  const [revokeTarget, setRevokeTarget] = React.useState<ProbeAgent>();
  const [configurationTarget, setConfigurationTarget] = React.useState<ProbeAgent>();
  const locationsQuery = useQuery({
    queryKey: ['synthetics', 'locations'],
    queryFn: syntheticsApi.listLocations,
  });
  const agentsQuery = useQuery({
    queryKey: ['synthetics', 'agents'],
    queryFn: syntheticsApi.listAllAgents,
  });
  const locations = React.useMemo(() => locationsQuery.data ?? [], [locationsQuery.data]);
  const agents = React.useMemo(() => agentsQuery.data ?? [], [agentsQuery.data]);
  const rows = React.useMemo(
    () => joinAgentLocations(agents, locations),
    [agents, locations],
  );
  const eligibleLocations = React.useMemo(
    () =>
      locations.filter(
        (location) =>
          location.scope === 'organization' &&
          location.execution === 'agent_pool' &&
          location.lifecycle === 'active',
      ),
    [locations],
  );
  const summary = React.useMemo(() => summarizeAgents(agents), [agents]);
  const query = searchParams.get('query') ?? '';
  const statusParam = searchParams.get('status');
  const statusFilter: AgentStatus | 'all' = AGENT_STATUSES.includes(statusParam as AgentStatus)
    ? (statusParam as AgentStatus)
    : 'all';
  const locationFilter = searchParams.get('location') ?? 'all';
  const filteredRows = React.useMemo(
    () =>
      filterAgentRows(rows, {
        query,
        status: statusFilter,
        locationId: locationFilter,
      }),
    [locationFilter, query, rows, statusFilter],
  );
  const selectedRow = rows.find(({ agent }) => agent.id === agentId);
  const refresh = () => queryClient.invalidateQueries({ queryKey: ['synthetics'] });
  const revoke = useMutation({
    mutationFn: syntheticsApi.revokeAgent,
    onSuccess: async () => {
      setRevokeTarget(undefined);
      navigate('/synthetics/agents', { replace: true });
      await refresh();
      toast.success(t('agents.revoked'));
    },
    onError: (error) => toast.error(toApiError(error).message),
  });
  const openConfiguration = (agent: ProbeAgent) => {
    navigate('/synthetics/agents', { replace: true });
    setConfigurationTarget(agent);
  };
  const configurationUpdated = async (updated: ProbeAgent) => {
    queryClient.setQueryData<ProbeAgent[]>(['synthetics', 'agents'], (current) =>
      current?.map((agent) => (agent.id === updated.id ? updated : agent)),
    );
    setConfigurationTarget(undefined);
    navigate(`/synthetics/agents/${updated.id}`, { replace: true });
    await refresh();
  };
  const setFilter = (key: 'query' | 'status' | 'location', value: string) => {
    const next = new URLSearchParams(searchParams);
    if (!value || value === 'all') next.delete(key);
    else next.set(key, value);
    setSearchParams(next, { replace: true });
  };

  return (
    <SyntheticsPage
      title={t('agents.title')}
      subtitle={t('agents.subtitle')}
      toolbar={
        <div className="flex items-center gap-2">
          <ChromeButton onClick={() => void refresh()} disabled={agentsQuery.isFetching}>
            <RefreshCw
              aria-hidden
              className={agentsQuery.isFetching ? 'h-3.5 w-3.5 animate-spin' : 'h-3.5 w-3.5'}
            />
            {t('actions.refresh')}
          </ChromeButton>
          {canManage && (
            <ChromeButton
              variant="primary"
              disabled={eligibleLocations.length === 0}
              disabledReason={t('agents.no_eligible_locations')}
              onClick={() => setRegisterOpen(true)}
            >
              <Plus aria-hidden className="h-3.5 w-3.5" />
              {t('actions.register_agent')}
            </ChromeButton>
          )}
        </div>
      }
      bodyClassName="space-y-0 pb-4 pt-2 lg:pb-6 lg:pt-2"
    >
      <WorkspaceBoundary
        pending={locationsQuery.isPending || agentsQuery.isPending}
        error={locationsQuery.error ?? agentsQuery.error}
        onRetry={() => void refresh()}
        flat
      >
        <SyntheticsCanvas>
          <SyntheticsKpiBand
            columns={5}
            items={[
              {
                label: t('agents.active_agents'),
                value: summary.active,
                sub: t('agents.registered_hint', {
                  count: summary.registered,
                  revoked: summary.revoked,
                }),
              },
              {
                label: t('states.online'),
                value: summary.online,
                sub: t('agents.online_hint', { count: summary.active }),
                tone: summary.online === summary.active ? 'good' : 'neutral',
              },
              {
                label: t('agents.attention'),
                value: summary.attention,
                sub: t('agents.attention_hint'),
                tone: summary.attention > 0 ? 'warn' : 'good',
              },
              {
                label: t('agents.concurrent_capacity'),
                value: `${summary.capacity.available}/${summary.capacity.maximum}`,
                sub: t('agents.available_slots'),
                tone: summary.capacity.maximum > 0 && summary.capacity.available === 0 ? 'danger' : 'neutral',
              },
              {
                label: t('agents.browser_capacity'),
                value: `${summary.browserCapacity.available}/${summary.browserCapacity.maximum}`,
                sub: t('agents.available_slots'),
              },
            ]}
          />

          {agents.length === 0 ? (
            <ProductState
              variant="empty"
              title={t('agents.no_agents')}
              description={t('agents.no_agents_hint')}
              className="rounded-none border-x-0 border-t-0 border-solid border-bd-0 bg-transparent"
              action={
                canManage && eligibleLocations.length > 0 ? (
                  <ChromeButton variant="primary" onClick={() => setRegisterOpen(true)}>
                    <Plus aria-hidden className="h-3.5 w-3.5" />
                    {t('actions.register_agent')}
                  </ChromeButton>
                ) : undefined
              }
            />
          ) : (
            <SyntheticsListSurface>
              <SyntheticsFilterBar className="grid grid-cols-1 gap-2 sm:grid-cols-[minmax(220px,1fr)_180px_200px]">
                <FormInput
                  aria-label={t('agents.search_placeholder')}
                  value={query}
                  onChange={(event) => setFilter('query', event.target.value)}
                  placeholder={t('agents.search_placeholder')}
                  className="h-11 text-base sm:h-9 sm:text-sm"
                />
                <FormSelect
                  ariaLabel={t('agents.all_statuses')}
                  value={statusFilter}
                  onChange={(value) => setFilter('status', value)}
                  className="h-11 text-base sm:h-9 sm:text-sm"
                  options={[
                    { value: 'all', label: t('agents.all_statuses') },
                    ...AGENT_STATUSES.map((status) => ({
                      value: status,
                      label: t(`states.${status}`),
                    })),
                  ]}
                />
                <FormSelect
                  ariaLabel={t('agents.all_locations')}
                  value={locationFilter}
                  onChange={(value) => setFilter('location', value)}
                  className="h-11 text-base sm:h-9 sm:text-sm"
                  options={[
                    { value: 'all', label: t('agents.all_locations') },
                    ...locations.map((location) => ({
                      value: location.id,
                      label: location.name,
                    })),
                  ]}
                />
              </SyntheticsFilterBar>
              <div className="overflow-x-auto">
                <DataTable
                  rows={filteredRows}
                  columns={agentColumns(t, i18n.language)}
                  rowKey={({ agent }) => agent.id}
                  onRowClick={({ agent }) => navigate(`/synthetics/agents/${agent.id}`)}
                  emptyLabel={t('agents.filtered_empty')}
                  className="min-w-[1080px] rounded-none border-0 bg-transparent"
                />
              </div>
            </SyntheticsListSurface>
          )}
        </SyntheticsCanvas>
      </WorkspaceBoundary>

      <AgentDetailDrawer
        row={selectedRow}
        canManage={canManage}
        onClose={() => navigate('/synthetics/agents', { replace: true })}
        onRequestEdit={openConfiguration}
        onRequestRevoke={setRevokeTarget}
      />
      <AgentConfigurationDrawer
        agent={configurationTarget}
        onClose={() => setConfigurationTarget(undefined)}
        onUpdated={configurationUpdated}
      />
      <AgentRegisterDrawer
        open={registerOpen}
        onOpenChange={setRegisterOpen}
        locations={eligibleLocations}
        onCreated={setInstructions}
      />
      <RegisterInstructionsDrawer
        instructions={instructions}
        onClose={() => setInstructions(undefined)}
      />
      <ConfirmDialog
        open={Boolean(revokeTarget)}
        onOpenChange={(open) => !open && setRevokeTarget(undefined)}
        title={t('actions.revoke')}
        description={
          revokeTarget
            ? t('agents.confirm_revoke', { name: agentDisplayName(revokeTarget) })
            : undefined
        }
        confirmLabel={t('actions.revoke')}
        destructive
        busy={revoke.isPending}
        onConfirm={() => revokeTarget && revoke.mutate(revokeTarget.id)}
      />
    </SyntheticsPage>
  );
}

function agentColumns(t: TFunction<'synthetics'>, locale: string): DataTableColumn<AgentRow>[] {
  return [
    {
      key: 'agent',
      header: t('agents.agent'),
      width: 230,
      cell: ({ agent }) => (
        <div className="flex min-w-0 items-center gap-2.5">
          <span className="grid h-8 w-8 shrink-0 place-items-center rounded-md bg-bg-3 text-tx-2">
            {agent.organization_id ? <Server aria-hidden className="h-4 w-4" /> : <Cpu aria-hidden className="h-4 w-4" />}
          </span>
          <div className="min-w-0">
            <div className="truncate font-strong text-tx-0">{agentDisplayName(agent)}</div>
            <div className="mt-0.5 truncate font-code text-type-micro text-tx-3">{agent.hostname}</div>
          </div>
        </div>
      ),
    },
    {
      key: 'status',
      header: t('agents.status'),
      width: 110,
      cell: ({ agent }) => <AgentStatusPill status={agent.status} />,
    },
    {
      key: 'location',
      header: t('agents.location'),
      width: 150,
      cell: ({ agent, location }) => (
        <div>
          <div className="truncate text-tx-1">{location?.name ?? agent.location_id}</div>
          {location && <div className="mt-0.5 font-code text-type-micro text-tx-3">{location.code}</div>}
        </div>
      ),
    },
    {
      key: 'version',
      header: t('agents.version'),
      width: 100,
      cell: ({ agent }) => (
        <div className="font-code text-xs text-tx-2">
          v{agent.agent_version}
          <div className="mt-0.5 text-type-micro text-tx-3">P{agent.protocol_version}</div>
        </div>
      ),
    },
    {
      key: 'capabilities',
      header: t('agents.capabilities'),
      width: 155,
      cell: ({ agent }) => (
        <span className="text-xs text-tx-2">
          {agent.capabilities.slice(0, 3).join(' · ')}
          {agent.capabilities.length > 3 ? ` +${agent.capabilities.length - 3}` : ''}
        </span>
      ),
    },
    {
      key: 'capacity',
      header: t('agents.capacity'),
      width: 145,
      cell: ({ agent }) => <CompactCapacity capacity={agent.capacity} />,
    },
    {
      key: 'heartbeat',
      header: t('agents.last_heartbeat'),
      width: 120,
      cell: ({ agent }) => formatRelativeTimestamp(agent.last_heartbeat_at, locale),
    },
    {
      key: 'certificate',
      header: t('agents.certificate'),
      width: 130,
      cell: ({ agent }) =>
        agent.certificate_expires_at
          ? formatRelativeTimestamp(agent.certificate_expires_at, locale)
          : t('agents.embedded_identity'),
    },
  ];
}

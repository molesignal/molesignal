import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import type { TFunction } from 'i18next';
import { Cpu, Plus, Search, Server } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useParams, useSearchParams } from 'react-router-dom';

import { ConfirmDialog, DataTable, type DataTableColumn } from '@/admin';
import * as syntheticsApi from '@/api/synthetics';
import type { ProbeAgent } from '@/api/synthetics';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ProductState } from '@/product/states';
import { ChromeButton } from '@/shell/chrome';
import { FormInput, FormSelect } from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';

import { SYNTHETICS_REFRESH_MILLIS } from '../data';
import { SyntheticsList, SyntheticsPage, useLoadState } from '../Frame';
import { formatRelativeTimestamp } from '../model';
import { AgentConfigurationDrawer } from './AgentConfigurationDrawer';
import { AgentDetailDrawer } from './AgentDetailDrawer';
import { AgentStatusPill, CompactCapacity } from './components';
import {
  AGENT_STATUSES,
  agentDisplayName,
  filterAgentRows,
  joinAgentLocations,
  type AgentRow,
  type AgentStatus,
} from './model';
import {
  AgentRegisterDrawer,
  RegisterInstructionsDrawer,
} from './RegisterDrawers';
import { AgentsStanding } from './Standing';
import { AgentTokenPanel } from './tokens/TokenPanel';

/**
 * The machines that run checks: whether they are up, what they can run, and how
 * much room they have.
 *
 * The tab opens with one statement about the fleet and lists the agents under
 * it. The tokens new agents register with come after: they are how the list
 * grows, not what a reader opens the tab to see.
 */
export function Agents() {
  const { t, i18n } = useTranslation('synthetics');
  const navigate = useNavigate();
  const { agentId } = useParams();
  const [searchParams, setSearchParams] = useSearchParams();
  const queryClient = useQueryClient();
  const manageAccess = useActionAccess({ permission: 'synthetics.locations.manage' });
  const canManage = manageAccess.allowed;
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
    refetchInterval: SYNTHETICS_REFRESH_MILLIS,
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

  const state = useLoadState({
    pending: locationsQuery.isPending || agentsQuery.isPending,
    error: locationsQuery.error ?? agentsQuery.error,
    onRetry: () => void refresh(),
  });
  const registerAccess = {
    disabled: manageAccess.disabled || eligibleLocations.length === 0,
    reason: manageAccess.reason ?? t('agents.no_eligible_locations'),
  };
  const registerAction = (
    <ChromeButton
      variant="primary"
      disabled={registerAccess.disabled}
      disabledReason={registerAccess.reason}
      onClick={() => setRegisterOpen(true)}
    >
      <Plus aria-hidden className="h-3.5 w-3.5" />
      {t('actions.register_agent')}
    </ChromeButton>
  );
  const none = !state && agents.length === 0;

  return (
    <>
      <SyntheticsPage
        state={state}
        // With no agent yet, the empty state below carries the way to the first one.
        action={none ? undefined : registerAction}
        lead={!state && !none ? <AgentsStanding agents={agents} /> : undefined}
        contentSurface={false}
      >
        {none ? (
          <ProductState
            variant="empty"
            title={t('agents.no_agents')}
            description={t('agents.no_agents_hint')}
            action={registerAction}
          />
        ) : (
          <SyntheticsList
            head={
              <>
                <label className="relative w-full min-w-0 sm:w-[300px]">
                  <span className="sr-only">{t('agents.search_placeholder')}</span>
                  <Search className="pointer-events-none absolute left-3 top-2.5 h-4 w-4 text-tx-3" />
                  <FormInput
                    value={query}
                    onChange={(event) => setFilter('query', event.target.value)}
                    placeholder={t('agents.search_placeholder')}
                    className="pl-9"
                  />
                </label>
                <FormSelect
                  ariaLabel={t('agents.status')}
                  className="w-full sm:w-[170px]"
                  value={statusFilter}
                  onChange={(value) => setFilter('status', value)}
                  options={[
                    { value: 'all', label: t('agents.all_statuses') },
                    ...AGENT_STATUSES.map((status) => ({
                      value: status,
                      label: t(`states.${status}`),
                    })),
                  ]}
                />
                <FormSelect
                  ariaLabel={t('agents.location')}
                  className="w-full sm:w-[200px]"
                  value={locationFilter}
                  onChange={(value) => setFilter('location', value)}
                  options={[
                    { value: 'all', label: t('agents.all_locations') },
                    ...locations.map((location) => ({
                      value: location.id,
                      label: location.name,
                    })),
                  ]}
                />
              </>
            }
          >
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
          </SyntheticsList>
        )}
        <AgentTokenPanel locations={eligibleLocations} canManage={canManage} />
      </SyntheticsPage>

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
    </>
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

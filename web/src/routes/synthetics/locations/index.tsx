import { useMutation, useQueries, useQuery, useQueryClient } from '@tanstack/react-query';
import { Plus } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useParams } from 'react-router-dom';

import { DataTable } from '@/admin';
import * as syntheticsApi from '@/api/synthetics';
import type { ProbeRegisterInstructions } from '@/api/synthetics';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ChromeButton } from '@/shell/chrome';
import { toast } from '@/shell/ui/sonner';
import { formatRelativeMicros } from '@/time/relative';

import { RegisterInstructionsDrawer } from '../agents/RegisterDrawers';
import { SYNTHETICS_REFRESH_MILLIS } from '../data';
import { SyntheticsPage, useLoadState } from '../Frame';
import { LocationHealthMark } from '../marks';
import { CreateLocationDrawer } from './CreateDrawer';
import { LocationDetailDrawer } from './DetailDrawer';

/**
 * Where checks run from: the public regions MoleSignal provides and the
 * private locations an organization adds behind its own firewall.
 *
 * Each row says whether the place can run checks now, which is what a reader
 * came to find out. A location is online or not, so that is the word used: the
 * "passed" of a check is not a state a place can be in.
 */
export function Locations() {
  const { t, i18n } = useTranslation('synthetics');
  const navigate = useNavigate();
  const { locationId } = useParams();
  const queryClient = useQueryClient();
  const manageAccess = useActionAccess({ permission: 'synthetics.locations.manage' });
  const [createOpen, setCreateOpen] = React.useState(false);
  const [registerInstructions, setRegisterInstructions] =
    React.useState<ProbeRegisterInstructions>();
  const locationsQuery = useQuery({
    queryKey: ['synthetics', 'locations'],
    queryFn: syntheticsApi.listLocations,
    refetchInterval: SYNTHETICS_REFRESH_MILLIS,
  });
  const locations = locationsQuery.data ?? [];
  const agentQueries = useQueries({
    queries: locations.map((location) => ({
      queryKey: ['synthetics', 'location', location.id, 'agents'],
      queryFn: () => syntheticsApi.listAgents(location.id),
      enabled: location.execution === 'agent_pool',
      refetchInterval: SYNTHETICS_REFRESH_MILLIS,
    })),
  });
  const agentsByLocation = new Map(
    locations.map((location, index) => [location.id, agentQueries[index]?.data ?? []]),
  );
  const selected = locations.find((location) => location.id === locationId);
  const refresh = () => queryClient.invalidateQueries({ queryKey: ['synthetics'] });
  const registerAgent = useMutation({
    mutationFn: (id: string) => syntheticsApi.createRegisterToken(id),
    onSuccess: setRegisterInstructions,
    onError: (error) => toast.error(toApiError(error).message),
  });
  const revoke = useMutation({
    mutationFn: syntheticsApi.revokeAgent,
    onSuccess: refresh,
    onError: (error) => toast.error(toApiError(error).message),
  });
  const locationLifecycle = useMutation({
    mutationFn: ({ id, lifecycle }: { id: string; lifecycle: 'active' | 'paused' }) =>
      syntheticsApi.setLocationLifecycle(id, lifecycle),
    onSuccess: refresh,
    onError: (error) => toast.error(toApiError(error).message),
  });
  const state = useLoadState({
    pending:
      locationsQuery.isPending ||
      locations.some(
        (location, index) =>
          location.execution === 'agent_pool' && Boolean(agentQueries[index]?.isPending),
      ),
    error: locationsQuery.error ?? agentQueries.find((query) => query.error)?.error,
    onRetry: () => void refresh(),
  });
  const nowMicros = Date.now() * 1000;

  return (
    <>
      <SyntheticsPage
        state={state}
        action={
          <ChromeButton
            variant="primary"
            disabled={manageAccess.disabled}
            disabledReason={manageAccess.reason}
            onClick={() => setCreateOpen(true)}
          >
            <Plus aria-hidden className="h-3.5 w-3.5" />
            {t('actions.create_location')}
          </ChromeButton>
        }
      >
        <div className="overflow-x-auto">
          <DataTable
            rows={locations}
            rowKey={(location) => location.id}
            onRowClick={(location) => navigate(`/synthetics/locations/${location.id}`)}
            emptyLabel={t('states.no_locations')}
            className="min-w-[820px] rounded-none border-0 bg-transparent"
            columns={[
              {
                key: 'name',
                header: t('locations.name'),
                cell: (location) => (
                  <div className="min-w-0 py-1">
                    <div className="flex min-w-0 items-center gap-[8px]">
                      <span className="truncate font-strong text-tx-0">{location.name}</span>
                      {/* A location runs unless it says otherwise. */}
                      {location.lifecycle === 'paused' && (
                        <span className="shrink-0 rounded-full bg-bg-3 px-[8px] py-[1px] text-xs text-tx-2">
                          {t('locations.paused_tag')}
                        </span>
                      )}
                    </div>
                    {location.description && (
                      <div className="mt-0.5 truncate text-xs text-tx-2">{location.description}</div>
                    )}
                  </div>
                ),
              },
              {
                key: 'code',
                header: t('locations.code'),
                width: 190,
                cell: (location) => (
                  <span className="font-code text-xs text-tx-1">{location.code}</span>
                ),
              },
              {
                key: 'kind',
                header: t('locations.kind'),
                width: 120,
                cell: (location) => (
                  <span className="text-tx-1">{t(`locations.${location.scope}`)}</span>
                ),
              },
              {
                key: 'runs_on',
                header: t('locations.runs_on'),
                width: 140,
                cell: (location) => (
                  <span className="text-tx-1">
                    {location.execution === 'embedded'
                      ? t('locations.built_in')
                      : t('locations.agents', {
                          count: agentsByLocation.get(location.id)?.length ?? 0,
                        })}
                  </span>
                ),
              },
              {
                key: 'health',
                header: t('locations.health'),
                width: 130,
                cell: (location) => <LocationHealthMark health={location.health} />,
              },
              {
                key: 'updated',
                header: t('locations.updated'),
                width: 150,
                cell: (location) => (
                  <span className="text-tx-2">
                    {formatRelativeMicros(location.updated_at, i18n.language, nowMicros)}
                  </span>
                ),
              },
            ]}
          />
        </div>
      </SyntheticsPage>

      <CreateLocationDrawer
        open={createOpen}
        onOpenChange={setCreateOpen}
        onCreated={async (location) => {
          await refresh();
          setCreateOpen(false);
          navigate(`/synthetics/locations/${location.id}`);
        }}
      />
      <LocationDetailDrawer
        location={selected}
        agents={selected ? (agentsByLocation.get(selected.id) ?? []) : []}
        canManage={manageAccess.allowed}
        onClose={() => navigate('/synthetics/locations', { replace: true })}
        onRegister={(id) => registerAgent.mutate(id)}
        registering={registerAgent.isPending}
        onRevoke={(id) => revoke.mutate(id)}
        onToggleLifecycle={(location) =>
          locationLifecycle.mutate({
            id: location.id,
            lifecycle: location.lifecycle === 'paused' ? 'active' : 'paused',
          })
        }
        lifecyclePending={locationLifecycle.isPending}
      />
      <RegisterInstructionsDrawer
        instructions={registerInstructions}
        onClose={() => setRegisterInstructions(undefined)}
      />
    </>
  );
}

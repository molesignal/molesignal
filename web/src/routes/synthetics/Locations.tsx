import { useMutation, useQueries, useQuery, useQueryClient } from '@tanstack/react-query';
import type { TFunction } from 'i18next';
import { Pause, Play, Plus, RefreshCw, ShieldCheck, Wifi, WifiOff } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate, useParams } from 'react-router-dom';

import { DataTable, type DataTableColumn } from '@/admin';
import * as syntheticsApi from '@/api/synthetics';
import type { ProbeAgent, ProbeRegisterInstructions, ProbeLocation } from '@/api/synthetics';
import { toApiError } from '@/lib/http';
import { hasPermission, useProductAccess } from '@/product/access';
import { ChromeButton } from '@/shell/chrome';
import {
  FormDrawer,
  FormField,
  FormInput,
  FormSection,
  FormSubmitFooter,
  FormTextarea,
} from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';

import { RegisterInstructionsDrawer } from './agents/RegisterDrawers';
import {
  StatePill,
  SyntheticsCanvas,
  SyntheticsKpiBand,
  SyntheticsListSurface,
  SyntheticsPage,
  WorkspaceBoundary,
} from './components';
import { formatRelativeTimestamp } from './model';

export function Locations() {
  const { t, i18n } = useTranslation('synthetics');
  const navigate = useNavigate();
  const { locationId } = useParams();
  const queryClient = useQueryClient();
  const access = useProductAccess();
  const canManage = hasPermission('synthetics.locations.manage', access);
  const [createOpen, setCreateOpen] = React.useState(false);
  const [registerInstructions, setRegisterInstructions] = React.useState<ProbeRegisterInstructions>();
  const locationsQuery = useQuery({ queryKey: ['synthetics', 'locations'], queryFn: syntheticsApi.listLocations });
  const locations = locationsQuery.data ?? [];
  const agentQueries = useQueries({
    queries: locations.map((location) => ({
      queryKey: ['synthetics', 'location', location.id, 'agents'],
      queryFn: () => syntheticsApi.listAgents(location.id),
      enabled: location.execution === 'agent_pool',
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
  const pending =
    locationsQuery.isPending ||
    locations.some(
      (location, index) =>
        location.execution === 'agent_pool' && Boolean(agentQueries[index]?.isPending),
    );
  const error = locationsQuery.error ?? agentQueries.find((query) => query.error)?.error;

  return (
    <SyntheticsPage
      title={t('locations.title')}
      subtitle={t('locations.subtitle')}
      toolbar={
        <div className="flex items-center gap-2">
          <ChromeButton onClick={() => void refresh()}>
            <RefreshCw aria-hidden className="h-3.5 w-3.5" />
            {t('actions.refresh')}
          </ChromeButton>
          {canManage && (
            <ChromeButton variant="primary" onClick={() => setCreateOpen(true)}>
              <Plus aria-hidden className="h-3.5 w-3.5" />
              {t('actions.create_location')}
            </ChromeButton>
          )}
        </div>
      }
      bodyClassName="space-y-0 pb-4 pt-2 lg:pb-6 lg:pt-2"
    >
      <WorkspaceBoundary pending={pending} error={error} onRetry={() => void refresh()} flat>
        <SyntheticsCanvas>
          <SyntheticsKpiBand
            columns={3}
            items={[
              { label: t('locations.platform'), value: locations.filter((location) => location.scope === 'platform').length },
              { label: t('locations.organization'), value: locations.filter((location) => location.scope === 'organization').length },
              { label: t('states.online'), value: locations.filter((location) => location.health === 'online').length, tone: 'good' },
            ]}
          />
          <SyntheticsListSurface>
            <div className="overflow-x-auto">
              <DataTable
                rows={locations}
                columns={locationColumns(agentsByLocation, i18n.language, t)}
                rowKey={(location) => location.id}
                onRowClick={(location) => navigate(`/synthetics/locations/${location.id}`)}
                emptyLabel={t('states.no_locations')}
                className="min-w-[860px] rounded-none border-0 bg-transparent"
              />
            </div>
          </SyntheticsListSurface>
        </SyntheticsCanvas>
      </WorkspaceBoundary>

      <CreateLocationDrawer open={createOpen} onOpenChange={setCreateOpen} onCreated={async (location) => { await refresh(); setCreateOpen(false); navigate(`/synthetics/locations/${location.id}`); }} />
      <LocationDetailDrawer
        location={selected}
        agents={selected ? agentsByLocation.get(selected.id) ?? [] : []}
        canManage={canManage}
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
    </SyntheticsPage>
  );
}

function locationColumns(agentsByLocation: Map<string, ProbeAgent[]>, locale: string, t: TFunction<'synthetics'>): DataTableColumn<ProbeLocation>[] {
  return [
    { key: 'name', header: t('locations.name'), width: 220, cell: (location) => <div><div className="font-strong text-tx-0">{location.name}</div><div className="mt-0.5 text-type-micro text-tx-3">{location.description}</div></div> },
    { key: 'code', header: t('locations.code'), cell: (location) => <span className="font-code text-xs">{location.code}</span> },
    { key: 'scope', header: t('locations.scope'), cell: (location) => t(`locations.${location.scope}`) },
    { key: 'execution', header: t('locations.execution'), cell: (location) => t(`locations.${location.execution}`) },
    { key: 'lifecycle', header: t('checks.columns.status'), cell: (location) => t(`states.${location.lifecycle}`) },
    { key: 'health', header: t('locations.health'), cell: (location) => <StatePill state={location.health === 'online' ? 'healthy' : location.health === 'degraded' ? 'degraded' : location.health === 'offline' ? 'failing' : 'unknown'} compact /> },
    { key: 'agents', header: t('locations.agents'), cell: (location) => agentsByLocation.get(location.id)?.length ?? (location.execution === 'embedded' ? 1 : 0) },
    { key: 'updated', header: t('locations.updated'), cell: (location) => formatRelativeTimestamp(location.updated_at, locale) },
  ];
}

function CreateLocationDrawer({ open, onOpenChange, onCreated }: { open: boolean; onOpenChange: (open: boolean) => void; onCreated: (location: ProbeLocation) => void }) {
  const { t } = useTranslation('synthetics');
  const [name, setName] = React.useState('');
  const [code, setCode] = React.useState('');
  const [description, setDescription] = React.useState('');
  const [allowedDomains, setAllowedDomains] = React.useState('');
  const [allowPrivate, setAllowPrivate] = React.useState(true);
  const create = useMutation({
    mutationFn: () => syntheticsApi.createLocation({
      name,
      code,
      description,
      egress_policy: {
        allowed_cidrs: [], denied_cidrs: [],
        allowed_domains: allowedDomains.split(',').map((value) => value.trim()).filter(Boolean),
        denied_domains: [], allowed_ports: [],
        allow_private_networks: allowPrivate, allow_loopback: false,
      },
    }),
    onSuccess: onCreated,
    onError: (error) => toast.error(toApiError(error).message),
  });
  React.useEffect(() => { if (open) { setName(''); setCode(''); setDescription(''); setAllowedDomains(''); setAllowPrivate(true); } }, [open]);
  return <FormDrawer open={open} onOpenChange={onOpenChange} title={t('locations.create_title')} subtitle={t('locations.create_subtitle')} footer={<FormSubmitFooter busy={create.isPending} invalid={!name.trim() || !/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(code)} onCancel={() => onOpenChange(false)} formId="create-synthetic-location" submitLabel={t('actions.create_location')} />}><form id="create-synthetic-location" onSubmit={(event) => { event.preventDefault(); create.mutate(); }}><FormSection><FormField label={t('locations.name')} required><FormInput value={name} onChange={(event) => setName(event.target.value)} /></FormField><FormField label={t('locations.code')} required><FormInput value={code} onChange={(event) => setCode(event.target.value.toLowerCase())} placeholder="sg-private-1" /></FormField><FormField label={t('locations.description')}><FormTextarea value={description} onChange={(event) => setDescription(event.target.value)} /></FormField><FormField label={t('locations.allowed_domains')} hint={t('locations.allowed_domains_hint')}><FormInput value={allowedDomains} onChange={(event) => setAllowedDomains(event.target.value)} /></FormField><label className="flex min-h-11 cursor-pointer items-center gap-3 rounded-md border border-bd-0 bg-bg-2 px-3 text-sm text-tx-1"><input type="checkbox" checked={allowPrivate} onChange={(event) => setAllowPrivate(event.target.checked)} className="h-4 w-4 accent-indigo" />{t('locations.allow_private')}</label></FormSection></form></FormDrawer>;
}

function LocationDetailDrawer({ location, agents, canManage, onClose, onRegister, registering, onRevoke, onToggleLifecycle, lifecyclePending }: { location: ProbeLocation | undefined; agents: ProbeAgent[]; canManage: boolean; onClose: () => void; onRegister: (id: string) => void; registering: boolean; onRevoke: (id: string) => void; onToggleLifecycle: (location: ProbeLocation) => void; lifecyclePending: boolean }) {
  const { t, i18n } = useTranslation('synthetics');
  const footer = location && canManage && !location.system_managed && location.lifecycle !== 'archived' ? <div className="flex items-center gap-2"><ChromeButton disabled={lifecyclePending} onClick={() => onToggleLifecycle(location)}>{location.lifecycle === 'paused' ? <Play className="h-3.5 w-3.5" /> : <Pause className="h-3.5 w-3.5" />}{location.lifecycle === 'paused' ? t('actions.resume') : t('actions.pause')}</ChromeButton>{location.execution === 'agent_pool' && <ChromeButton variant="primary" disabled={registering || location.lifecycle !== 'active'} onClick={() => onRegister(location.id)}><Plus className="h-3.5 w-3.5" />{t('actions.register_agent')}</ChromeButton>}</div> : undefined;
  return <FormDrawer open={Boolean(location)} onOpenChange={(open) => !open && onClose()} title={location?.name ?? t('locations.title')} subtitle={location ? `${location.code} · ${t(`locations.${location.scope}`)}` : undefined} footer={footer}>{location && <><div className="mb-6 grid grid-cols-2 gap-3"><LocationFact label={t('locations.health')} value={<StatePill state={location.health === 'online' ? 'healthy' : location.health === 'degraded' ? 'degraded' : location.health === 'offline' ? 'failing' : 'unknown'} />} /><LocationFact label={t('locations.execution')} value={t(`locations.${location.execution}`)} /></div><FormSection title={t('locations.agents')}>{agents.length === 0 ? <div className="rounded-md border border-dashed border-bd-1 px-4 py-6 text-center text-xs text-tx-3">{t('locations.no_agents')}</div> : agents.map((agent) => <div key={agent.id} className="rounded-md border border-bd-0 bg-bg-2 p-3"><div className="flex items-start justify-between gap-3"><div><div className="flex items-center gap-2 text-sm font-strong text-tx-0">{agent.status === 'online' ? <Wifi className="h-3.5 w-3.5 text-green" /> : <WifiOff className="h-3.5 w-3.5 text-tx-3" />}{agent.name || agent.hostname}</div><div className="mt-1 text-xs text-tx-3">{agent.hostname} · v{agent.agent_version} · {formatRelativeTimestamp(agent.last_heartbeat_at, i18n.language)}</div></div>{canManage && agent.status !== 'revoked' && <ChromeButton onClick={() => onRevoke(agent.id)}>{t('actions.revoke')}</ChromeButton>}</div><div className="mt-3 flex flex-wrap gap-1.5">{agent.capabilities.map((capability) => <span key={capability} className="rounded-full border border-bd-0 bg-bg-1 px-2 py-0.5 text-type-micro uppercase text-tx-2">{capability}</span>)}</div></div>)}</FormSection><FormSection title={t('locations.egress_policy')}><div className="rounded-md border border-bd-0 bg-bg-2 p-3 text-xs text-tx-2"><div className="flex items-center gap-2"><ShieldCheck className="h-4 w-4 text-indigo-soft" />{location.egress_policy.allow_private_networks ? t('locations.allow_private') : t('locations.public_only')}</div>{location.egress_policy.allowed_domains.length > 0 && <div className="mt-2 font-code">{location.egress_policy.allowed_domains.join(', ')}</div>}</div></FormSection></>}</FormDrawer>;
}

function LocationFact({ label, value }: { label: React.ReactNode; value: React.ReactNode }) { return <div className="rounded-md border border-bd-0 bg-bg-2 p-3"><div className="text-type-micro uppercase tracking-wide text-tx-3">{label}</div><div className="mt-2 text-sm font-strong text-tx-0">{value}</div></div>; }

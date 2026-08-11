import { ExternalLink, Pencil, ShieldCheck, ShieldX } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { ProbeAgent } from '@/api/synthetics';
import { ChromeButton } from '@/shell/chrome';
import { FormDrawer, FormSection } from '@/shell/FormDrawer';

import { formatRelativeTimestamp, formatTimestamp } from '../model';
import { AgentStatusPill, CapacityMeter } from './components';
import { agentDisplayName, type AgentRow } from './model';

export function AgentDetailDrawer({
  row,
  canManage,
  onClose,
  onRequestEdit,
  onRequestRevoke,
}: {
  row: AgentRow | undefined;
  canManage: boolean;
  onClose: () => void;
  onRequestEdit: (agent: ProbeAgent) => void;
  onRequestRevoke: (agent: ProbeAgent) => void;
}) {
  const { t, i18n } = useTranslation('synthetics');
  const agent = row?.agent;
  const canRevoke = Boolean(
    canManage && agent?.organization_id && agent.status !== 'revoked',
  );
  const footer = canManage && agent ? (
    <>
      {canRevoke && (
        <ChromeButton className="text-red-soft" onClick={() => onRequestRevoke(agent)}>
          <ShieldX aria-hidden className="h-3.5 w-3.5" />
          {t('actions.revoke')}
        </ChromeButton>
      )}
      <ChromeButton variant="primary" onClick={() => onRequestEdit(agent)}>
        <Pencil aria-hidden className="h-3.5 w-3.5" />
        {t('actions.edit_configuration')}
      </ChromeButton>
    </>
  ) : undefined;

  return (
    <FormDrawer
      open={Boolean(row)}
      onOpenChange={(open) => !open && onClose()}
      title={agent ? agentDisplayName(agent) : t('agents.title')}
      subtitle={agent?.hostname}
      footer={footer}
    >
      {agent && (
        <>
          <div className="mb-6 grid grid-cols-1 gap-3 sm:grid-cols-2">
            <AgentFact label={t('agents.status')} value={<AgentStatusPill status={agent.status} />} />
            <AgentFact
              label={t('agents.location')}
              value={
                row.location ? (
                  <Link
                    to={`/synthetics/locations/${row.location.id}`}
                    className="inline-flex items-center gap-1 rounded text-indigo-soft hover:text-tx-0 focus-visible:bg-bg-3"
                  >
                    {row.location.name}
                    <ExternalLink aria-hidden className="h-3 w-3" />
                  </Link>
                ) : (
                  agent.location_id
                )
              }
            />
            <AgentFact label={t('agents.version')} value={`v${agent.agent_version}`} />
            <AgentFact
              label={t('agents.management')}
              value={t(
                agent.organization_id
                  ? 'agents.organization_managed'
                  : 'agents.platform_managed',
              )}
            />
          </div>

          <FormSection title={t('agents.capacity')}>
            <div className="space-y-5 rounded-md border border-bd-0 bg-bg-2 p-4">
              <CapacityMeter
                label={t('agents.concurrent_capacity')}
                available={agent.capacity.available}
                maximum={agent.capacity.max_concurrent}
              />
              <CapacityMeter
                label={t('agents.browser_capacity')}
                available={agent.capacity.available_browser}
                maximum={agent.capacity.max_browser_concurrent}
              />
            </div>
          </FormSection>

          <FormSection title={t('agents.capabilities')}>
            <div className="flex flex-wrap gap-2">
              {agent.capabilities.map((capability) => (
                <span
                  key={capability}
                  className="rounded-full border border-bd-0 bg-bg-2 px-2.5 py-1 text-type-micro uppercase text-tx-1"
                >
                  {capability}
                </span>
              ))}
            </div>
          </FormSection>

          <FormSection title={t('agents.identity')}>
            <div className="divide-y divide-bd-0 overflow-hidden rounded-md border border-bd-0 bg-bg-2">
              <DetailRow label={t('agents.agent_id')} value={agent.id} code />
              <DetailRow label={t('agents.protocol')} value={String(agent.protocol_version)} />
              <DetailRow
                label={t('agents.last_heartbeat')}
                value={
                  agent.last_heartbeat_at
                    ? `${formatTimestamp(agent.last_heartbeat_at, i18n.language)} · ${formatRelativeTimestamp(agent.last_heartbeat_at, i18n.language)}`
                    : t('agents.never')
                }
              />
              <DetailRow
                label={t('agents.last_result_sequence')}
                value={agent.last_result_sequence.toLocaleString(i18n.language)}
              />
              <DetailRow
                label={t('agents.certificate_serial')}
                value={agent.certificate_serial ?? t('agents.certificate_none')}
                code={Boolean(agent.certificate_serial)}
              />
              <DetailRow
                label={t('agents.certificate_expiry')}
                value={formatTimestamp(agent.certificate_expires_at, i18n.language)}
              />
            </div>
          </FormSection>

          <FormSection title={t('agents.labels')}>
            {Object.keys(agent.labels).length > 0 ? (
              <div className="divide-y divide-bd-0 overflow-hidden rounded-md border border-bd-0 bg-bg-2">
                {Object.entries(agent.labels).map(([key, value]) => (
                  <DetailRow key={key} label={key} value={value} code />
                ))}
              </div>
            ) : (
              <div className="rounded-md border border-dashed border-bd-1 px-4 py-6 text-center text-xs text-tx-3">
                {t('agents.no_labels')}
              </div>
            )}
          </FormSection>

          {!agent.organization_id && (
            <div className="flex gap-3 rounded-md border border-bd-0 bg-bg-2 p-3 text-xs leading-relaxed text-tx-2">
              <ShieldCheck aria-hidden className="mt-0.5 h-4 w-4 shrink-0 text-indigo-soft" />
              {t('agents.platform_agent_hint')}
            </div>
          )}
        </>
      )}
    </FormDrawer>
  );
}

function AgentFact({ label, value }: { label: React.ReactNode; value: React.ReactNode }) {
  return (
    <div className="rounded-md border border-bd-0 bg-bg-2 p-3">
      <div className="text-type-micro uppercase tracking-wide text-tx-3">{label}</div>
      <div className="mt-2 min-w-0 truncate text-sm font-strong text-tx-0">{value}</div>
    </div>
  );
}

function DetailRow({
  label,
  value,
  code = false,
}: {
  label: React.ReactNode;
  value: React.ReactNode;
  code?: boolean;
}) {
  return (
    <div className="grid grid-cols-[minmax(112px,0.4fr)_minmax(0,1fr)] gap-3 px-3 py-2.5 text-xs">
      <div className="text-tx-3">{label}</div>
      <div className={`min-w-0 break-all text-tx-1 ${code ? 'font-code' : ''}`}>{value}</div>
    </div>
  );
}

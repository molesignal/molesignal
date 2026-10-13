import { Pause, Play, Plus, ShieldCheck } from 'lucide-react';
import { useTranslation } from 'react-i18next';

import type { ProbeAgent, ProbeLocation } from '@/api/synthetics';
import { ChromeButton } from '@/shell/chrome';
import { FormDrawer, FormSection } from '@/shell/FormDrawer';
import { formatRelativeMicros } from '@/time/relative';

import { AgentStatusPill } from '../agents/components';
import { LocationHealthMark } from '../marks';

/**
 * One location: whether it can run checks, the agents that run them there, and
 * what those agents are allowed to reach.
 */
export function LocationDetailDrawer({
  location,
  agents,
  canManage,
  onClose,
  onRegister,
  registering,
  onRevoke,
  onToggleLifecycle,
  lifecyclePending,
}: {
  location: ProbeLocation | undefined;
  agents: readonly ProbeAgent[];
  canManage: boolean;
  onClose: () => void;
  onRegister: (id: string) => void;
  registering: boolean;
  onRevoke: (id: string) => void;
  onToggleLifecycle: (location: ProbeLocation) => void;
  lifecyclePending: boolean;
}) {
  const { t, i18n } = useTranslation('synthetics');
  const nowMicros = Date.now() * 1000;
  const manageable =
    location && canManage && !location.system_managed && location.lifecycle !== 'archived';

  return (
    <FormDrawer
      open={Boolean(location)}
      onOpenChange={(open) => !open && onClose()}
      title={location?.name ?? t('tabs.locations')}
      subtitle={location ? `${location.code} · ${t(`locations.${location.scope}`)}` : undefined}
      footer={
        manageable ? (
          <div className="flex items-center gap-2">
            <ChromeButton disabled={lifecyclePending} onClick={() => onToggleLifecycle(location)}>
              {location.lifecycle === 'paused' ? (
                <Play className="h-3.5 w-3.5" />
              ) : (
                <Pause className="h-3.5 w-3.5" />
              )}
              {t(location.lifecycle === 'paused' ? 'actions.resume' : 'actions.pause')}
            </ChromeButton>
            {location.execution === 'agent_pool' && (
              <ChromeButton
                variant="primary"
                disabled={registering || location.lifecycle !== 'active'}
                onClick={() => onRegister(location.id)}
              >
                <Plus className="h-3.5 w-3.5" />
                {t('actions.register_agent')}
              </ChromeButton>
            )}
          </div>
        ) : undefined
      }
    >
      {location && (
        <>
          <dl className="mb-6 grid grid-cols-2 gap-x-[24px] gap-y-[12px] font-sans text-sm">
            <div>
              <dt className="text-xs text-tx-2">{t('locations.health')}</dt>
              <dd className="mt-[4px]">
                <LocationHealthMark health={location.health} />
              </dd>
            </div>
            <div>
              <dt className="text-xs text-tx-2">{t('locations.runs_on')}</dt>
              <dd className="mt-[4px] text-tx-0">{t(`locations.${location.execution}`)}</dd>
            </div>
          </dl>

          <FormSection title={t('locations.agents_title')}>
            {agents.length === 0 ? (
              <p className="py-[6px] font-sans text-xs text-tx-2">{t('locations.no_agents')}</p>
            ) : (
              <ul className="divide-y divide-bd-0">
                {agents.map((agent) => (
                  <li key={agent.id} className="py-[10px]">
                    <div className="flex items-start justify-between gap-3">
                      <div className="min-w-0">
                        <div className="flex min-w-0 items-center gap-[8px]">
                          <span className="truncate font-sans text-sm font-strong text-tx-0">
                            {agent.name || agent.hostname}
                          </span>
                          <AgentStatusPill status={agent.status} />
                        </div>
                        <div className="mt-[2px] truncate font-sans text-xs text-tx-2">
                          {[
                            agent.hostname,
                            `v${agent.agent_version}`,
                            agent.last_heartbeat_at
                              ? formatRelativeMicros(agent.last_heartbeat_at, i18n.language, nowMicros)
                              : undefined,
                          ]
                            .filter(Boolean)
                            .join(' · ')}
                        </div>
                        <div className="mt-[4px] truncate font-sans text-xs text-tx-3">
                          {agent.capabilities.map((capability) => t(`kinds.${capability}`)).join(' · ')}
                        </div>
                      </div>
                      {canManage && agent.status !== 'revoked' && (
                        <ChromeButton size="sm" onClick={() => onRevoke(agent.id)}>
                          {t('actions.revoke')}
                        </ChromeButton>
                      )}
                    </div>
                  </li>
                ))}
              </ul>
            )}
          </FormSection>

          <FormSection title={t('locations.egress_policy')}>
            <div className="flex items-start gap-[8px] font-sans text-sm text-tx-1">
              <ShieldCheck aria-hidden className="mt-[2px] h-4 w-4 shrink-0 text-tx-3" />
              <div className="min-w-0">
                <div>
                  {t(
                    location.egress_policy.allow_private_networks
                      ? 'locations.allow_private'
                      : 'locations.public_only',
                  )}
                </div>
                {location.egress_policy.allowed_domains.length > 0 && (
                  <div className="mt-[4px] break-all font-code text-xs text-tx-2">
                    {location.egress_policy.allowed_domains.join(', ')}
                  </div>
                )}
              </div>
            </div>
          </FormSection>
        </>
      )}
    </FormDrawer>
  );
}

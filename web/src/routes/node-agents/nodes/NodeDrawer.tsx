import { Pause, Pencil, Play, PlaySquare, ShieldX } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import type { NodeAgent } from '@/api/nodeAgents';
import { useTimeFormatter } from '@/lib/time';
import { ChromeButton } from '@/shell/chrome';
import { FormDrawer, FormSection } from '@/shell/FormDrawer';
import { JsonView } from '@/shell/JsonEditor';
import { formatRelativeMicros } from '@/time/relative';

import { DetailRow, DetailRows, NodeStatusMark } from '../Frame';
import { nodeName } from '../model';

export interface NodeActions {
  canManage: boolean;
  canRevoke: boolean;
  canRunTasks: boolean;
  onDrain: (node: NodeAgent) => void;
  onResume: (node: NodeAgent) => void;
  onRevoke: (node: NodeAgent) => void;
  onEditConfig: (node: NodeAgent) => void;
  onRunTask: (node: NodeAgent) => void;
}

/** One node: where it stands, what it runs, what it was told, and what can be done to it. */
export function NodeDrawer({
  node,
  actions,
  onClose,
}: {
  node: NodeAgent | undefined;
  actions: NodeActions;
  onClose: () => void;
}) {
  const { t, i18n } = useTranslation('infrastructure');
  const time = useTimeFormatter();
  const revoked = node?.status === 'revoked';
  const draining = Boolean(node?.drainRequestedAt);

  const footer = node && !revoked ? (
    <>
      {actions.canRevoke && (
        <ChromeButton className="text-red" onClick={() => actions.onRevoke(node)}>
          <ShieldX aria-hidden className="h-3.5 w-3.5" />
          {t('node_agents.actions.revoke')}
        </ChromeButton>
      )}
      {actions.canManage && (
        <ChromeButton onClick={() => (draining ? actions.onResume(node) : actions.onDrain(node))}>
          {draining ? <Play aria-hidden className="h-3.5 w-3.5" /> : <Pause aria-hidden className="h-3.5 w-3.5" />}
          {t(draining ? 'node_agents.actions.resume' : 'node_agents.actions.drain')}
        </ChromeButton>
      )}
      {actions.canManage && (
        <ChromeButton onClick={() => actions.onEditConfig(node)}>
          <Pencil aria-hidden className="h-3.5 w-3.5" />
          {t('node_agents.actions.edit_config')}
        </ChromeButton>
      )}
      {actions.canRunTasks && !draining && (
        <ChromeButton variant="primary" onClick={() => actions.onRunTask(node)}>
          <PlaySquare aria-hidden className="h-3.5 w-3.5" />
          {t('node_agents.actions.run_task')}
        </ChromeButton>
      )}
    </>
  ) : undefined;

  return (
    <FormDrawer
      open={Boolean(node)}
      onOpenChange={(open) => !open && onClose()}
      title={node ? nodeName(node) : t('node_agents.tabs.nodes')}
      subtitle={node?.hostId}
      footer={footer}
    >
      {node && (
        <>
          <FormSection title={t('node_agents.node.state')}>
            <DetailRows>
              <DetailRow label={t('node_agents.columns.status')}>
                <NodeStatusMark status={node.status} />
              </DetailRow>
              <DetailRow label={t('node_agents.columns.heartbeat')}>
                {node.lastHeartbeatAt
                  ? `${time.micros(node.lastHeartbeatAt)} · ${formatRelativeMicros(node.lastHeartbeatAt, i18n.language)}`
                  : t('node_agents.never')}
              </DetailRow>
              <DetailRow label={t('node_agents.node.health')}>
                {t(`node_agents.health.${node.health.state}`)}
                {node.health.messages.length > 0 && ` · ${node.health.messages.join('; ')}`}
              </DetailRow>
              {node.drainDeadline && (
                <DetailRow label={t('node_agents.node.drain_deadline')}>{time.micros(node.drainDeadline)}</DetailRow>
              )}
              {node.revokedAt && (
                <DetailRow label={t('node_agents.node.revoked')}>
                  {time.micros(node.revokedAt)} · {node.revokeReason}
                </DetailRow>
              )}
            </DetailRows>
          </FormSection>

          <FormSection title={t('node_agents.node.agent')}>
            <DetailRows>
              <DetailRow label={t('node_agents.columns.version')} code>
                v{node.agentVersion} · P{node.protocolVersion}
              </DetailRow>
              <DetailRow label={t('node_agents.columns.task_kinds')}>
                {node.capabilities.taskKinds.length > 0
                  ? node.capabilities.taskKinds.map((kind) => t(`node_agents.kind.${kind}`)).join(', ')
                  : t('node_agents.none')}
              </DetailRow>
              <DetailRow label={t('node_agents.node.max_running')}>{node.capabilities.maxRunningTasks}</DetailRow>
              <DetailRow label={t('node_agents.node.node_id')} code>{node.id}</DetailRow>
              <DetailRow label={t('node_agents.node.certificate')} code>
                {node.certificateSerial} · {time.micros(node.certificateExpiresAt, false)}
              </DetailRow>
              <DetailRow label={t('node_agents.node.enrolled')}>{time.micros(node.enrolledAt)}</DetailRow>
            </DetailRows>
          </FormSection>

          {node.collectorStates.length > 0 && (
            <FormSection title={t('node_agents.node.collectors')}>
              <DetailRows>
                {node.collectorStates.map((collector) => (
                  <DetailRow key={collector.name} label={collector.name}>
                    {collector.state}
                    {collector.message && ` · ${collector.message}`}
                  </DetailRow>
                ))}
              </DetailRows>
            </FormSection>
          )}

          <FormSection title={t('node_agents.node.config')}>
            <DetailRows>
              <DetailRow label={t('node_agents.node.config_revision')}>
                {t('node_agents.node.config_revisions', {
                  applied: node.appliedConfigRevision,
                  desired: node.desiredConfig?.revision ?? 0,
                })}
              </DetailRow>
              {node.configStatus && (
                <DetailRow label={t('node_agents.node.config_status')}>
                  {t(`node_agents.config_state.${node.configStatus.state}`, {
                    revision: node.configStatus.revision,
                  })}
                  {node.configStatus.reason && ` · ${node.configStatus.reason}`}
                </DetailRow>
              )}
            </DetailRows>
            {node.desiredConfig && <JsonView value={node.desiredConfig.config} maxHeight={240} className="mt-2" />}
          </FormSection>

          {Object.keys(node.labels).length > 0 && (
            <FormSection title={t('node_agents.node.labels')}>
              <DetailRows>
                {Object.entries(node.labels).map(([key, value]) => (
                  <DetailRow key={key} label={key} code>
                    {value}
                  </DetailRow>
                ))}
              </DetailRows>
            </FormSection>
          )}

          <Link
            to={`/node-agents/tasks?nodeId=${encodeURIComponent(node.id)}`}
            className="text-xs text-indigo-soft hover:text-tx-0"
          >
            {t('node_agents.node.show_tasks')}
          </Link>
        </>
      )}
    </FormDrawer>
  );
}

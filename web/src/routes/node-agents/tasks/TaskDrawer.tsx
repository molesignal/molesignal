import { CircleStop } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { isActiveTask, type NodeTask } from '@/api/nodeAgents';
import { useTimeFormatter } from '@/lib/time';
import { ChromeButton } from '@/shell/chrome';
import { FormDrawer, FormSection } from '@/shell/FormDrawer';
import { JsonView } from '@/shell/JsonEditor';

import { DetailRow, DetailRows, TaskStateMark } from '../Frame';
import { taskSubject } from '../model';

/** One task: what it asked, where it stands, and what the node reported. */
export function TaskDrawer({
  task,
  nodeName,
  canCancel,
  cancelling,
  onCancel,
  onClose,
}: {
  task: NodeTask | undefined;
  nodeName: string | undefined;
  canCancel: boolean;
  cancelling: boolean;
  onCancel: (task: NodeTask) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation('infrastructure');
  const time = useTimeFormatter();
  const cancellable = Boolean(task && isActiveTask(task) && !task.cancelRequestedAt);

  const footer = task && canCancel && cancellable ? (
    <ChromeButton className="text-red" disabled={cancelling} onClick={() => onCancel(task)}>
      <CircleStop aria-hidden className="h-3.5 w-3.5" />
      {t('node_agents.actions.cancel_task')}
    </ChromeButton>
  ) : undefined;

  return (
    <FormDrawer
      open={Boolean(task)}
      onOpenChange={(open) => !open && onClose()}
      title={task ? taskSubject(task) : t('node_agents.tabs.tasks')}
      subtitle={task?.id}
      footer={footer}
    >
      {task && (
        <>
          <FormSection title={t('node_agents.task.state')}>
            <DetailRows>
              <DetailRow label={t('node_agents.columns.status')}>
                <TaskStateMark state={task.state} />
                {task.cancelRequestedAt && ` · ${t('node_agents.task.cancel_requested')}`}
              </DetailRow>
              <DetailRow label={t('node_agents.task.node')}>
                <Link to={`/node-agents/nodes/${task.nodeId}`} className="text-indigo-soft hover:text-tx-0">
                  {nodeName ?? task.nodeId}
                </Link>
              </DetailRow>
              <DetailRow label={t('node_agents.task.kind')}>{t(`node_agents.kind.${task.kind}`)}</DetailRow>
              <DetailRow label={t('node_agents.task.attempt')}>
                {t('node_agents.task.attempt_of', { attempt: task.attempt, max: task.maxAttempts })}
              </DetailRow>
              <DetailRow label={t('node_agents.task.created')}>{time.micros(task.createdAt)}</DetailRow>
              <DetailRow label={t('node_agents.task.deadline')}>{time.micros(task.deadline)}</DetailRow>
              {task.leaseExpiresAt && (
                <DetailRow label={t('node_agents.task.lease')}>{time.micros(task.leaseExpiresAt)}</DetailRow>
              )}
              {task.finishedAt && (
                <DetailRow label={t('node_agents.task.finished')}>{time.micros(task.finishedAt)}</DetailRow>
              )}
              {task.failure && <DetailRow label={t('node_agents.task.failure')}>{task.failure}</DetailRow>}
            </DetailRows>
          </FormSection>

          {task.result && (
            <FormSection title={t('node_agents.task.result')}>
              <DetailRows>
                <DetailRow label={t('node_agents.task.result_status')}>
                  {t(`node_agents.task_state.${task.result.status}`)}
                </DetailRow>
                <DetailRow label={t('node_agents.task.result_id')} code>
                  {task.result.resultId}
                </DetailRow>
                <DetailRow label={t('node_agents.task.received')}>{time.micros(task.result.receivedAt)}</DetailRow>
                {task.result.summary && (
                  <DetailRow label={t('node_agents.task.summary')} code>
                    {task.result.summary}
                  </DetailRow>
                )}
                {task.result.artifactRefs.map((reference) => (
                  <DetailRow key={reference} label={t('node_agents.task.artifact')} code>
                    {reference}
                  </DetailRow>
                ))}
              </DetailRows>
            </FormSection>
          )}

          <FormSection title={t('node_agents.task.spec')}>
            <JsonView value={task.spec} maxHeight={320} />
          </FormSection>
        </>
      )}
    </FormDrawer>
  );
}

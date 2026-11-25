import { useMutation } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as nodeAgentsApi from '@/api/nodeAgents';
import type { NodeAgent, NodeTask, NodeTaskKind } from '@/api/nodeAgents';
import { toApiError } from '@/lib/http';
import { FormDrawer, FormField, FormSection, FormSelect, FormSubmitFooter } from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';

import { durationLabel } from '../Frame';
import { nodeName } from '../model';
import { buildSpec, defaultDraft, type SpecDraft } from './spec';
import { SpecFields } from './SpecFields';

const TIMEOUT_MINUTES = [5, 15, 60, 360, 1440] as const;
const ATTEMPTS = [1, 2, 3, 5] as const;

/**
 * A diagnostic task for one node: the kind among those the node runs, its spec, how long it may
 * take and how often it is offered again. The quota decides whether it is taken.
 */
export function RunTaskDrawer({
  open,
  nodes,
  initialNodeId,
  onClose,
  onCreated,
}: {
  open: boolean;
  /** The nodes a task can go to: enrolled, not revoked, not draining. */
  nodes: readonly NodeAgent[];
  initialNodeId?: string | undefined;
  onClose: () => void;
  onCreated: (task: NodeTask) => void;
}) {
  const { t } = useTranslation('infrastructure');
  const [nodeId, setNodeId] = React.useState('');
  const node = nodes.find((candidate) => candidate.id === nodeId);
  const kinds = node?.capabilities.taskKinds ?? [];
  const [kind, setKind] = React.useState<NodeTaskKind>('tool');
  const [draft, setDraft] = React.useState<SpecDraft>(defaultDraft);
  const [timeout, setTimeoutMinutes] = React.useState('60');
  const [attempts, setAttempts] = React.useState('3');

  // Every opening starts from the node it was opened on and the first kind it runs.
  React.useEffect(() => {
    if (!open) return;
    const start = nodes.find((candidate) => candidate.id === initialNodeId) ?? nodes[0];
    const startKind = start?.capabilities.taskKinds[0] ?? 'tool';
    setNodeId(start?.id ?? '');
    setKind(startKind);
    setDraft(defaultDraft());
    setTimeoutMinutes('60');
    setAttempts('3');
  }, [initialNodeId, nodes, open]);

  const built = buildSpec(kind, draft);
  const problem = 'problem' in built ? built : undefined;

  const create = useMutation({
    mutationFn: (spec: Record<string, unknown>) =>
      nodeAgentsApi.createTask({
        nodeId,
        kind,
        spec,
        timeoutSeconds: Number(timeout) * 60,
        maxAttempts: Number(attempts),
      }),
    onSuccess: (task) => {
      toast.success(t('node_agents.run.created'));
      onCreated(task);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  const chooseNode = (id: string) => {
    setNodeId(id);
    const next = nodes.find((candidate) => candidate.id === id)?.capabilities.taskKinds ?? [];

    if (!next.includes(kind) && next[0]) setKind(next[0]);
  };

  return (
    <FormDrawer
      open={open}
      onOpenChange={(next) => !next && onClose()}
      title={t('node_agents.run.title')}
      subtitle={t('node_agents.run.subtitle')}
      footer={
        <FormSubmitFooter
          busy={create.isPending}
          invalid={!node || !kinds.includes(kind) || Boolean(problem)}
          onCancel={onClose}
          formId="run-node-task"
          submitLabel={t('node_agents.actions.run_task')}
        />
      }
    >
      <form
        id="run-node-task"
        onSubmit={(event) => {
          event.preventDefault();
          if ('spec' in built && node) create.mutate(built.spec);
        }}
      >
        <FormSection>
          <FormField label={t('node_agents.run.node')} required>
            <FormSelect
              value={nodeId}
              onChange={chooseNode}
              placeholder={t('node_agents.run.no_nodes')}
              options={nodes.map((candidate) => ({ value: candidate.id, label: nodeName(candidate) }))}
            />
          </FormField>
          <FormField label={t('node_agents.run.kind')} required hint={t('node_agents.run.kind_hint')}>
            <FormSelect
              value={kind}
              onChange={(value) => setKind(value as NodeTaskKind)}
              options={kinds.map((value) => ({ value, label: t(`node_agents.kind.${value}`) }))}
            />
          </FormField>
        </FormSection>
        <FormSection title={t('node_agents.run.spec')}>
          <SpecFields kind={kind} draft={draft} problem={problem} onChange={setDraft} />
        </FormSection>
        <FormSection title={t('node_agents.run.limits')}>
          <FormField label={t('node_agents.run.timeout')} hint={t('node_agents.run.timeout_hint')}>
            <FormSelect
              value={timeout}
              onChange={setTimeoutMinutes}
              options={TIMEOUT_MINUTES.map((value) => ({
                value: String(value),
                label: durationLabel(t, value),
              }))}
            />
          </FormField>
          <FormField label={t('node_agents.run.attempts')} hint={t('node_agents.run.attempts_hint')}>
            <FormSelect
              value={attempts}
              onChange={setAttempts}
              options={ATTEMPTS.map((value) => ({ value: String(value), label: String(value) }))}
            />
          </FormField>
        </FormSection>
      </form>
    </FormDrawer>
  );
}

import { useMutation } from '@tanstack/react-query';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import * as nodeAgentsApi from '@/api/nodeAgents';
import type { NodeAgent } from '@/api/nodeAgents';
import { toApiError } from '@/lib/http';
import {
  FormDrawer,
  FormField,
  FormSection,
  FormSelect,
  FormSubmitFooter,
  FormTextarea,
} from '@/shell/FormDrawer';
import { toast } from '@/shell/ui/sonner';

import { durationLabel } from '../Frame';
import { nodeName } from '../model';

const DRAIN_MINUTES = [5, 10, 30, 60, 360] as const;

/** Stop new tasks on a node and give running ones until the deadline. */
export function DrainDrawer({
  node,
  onClose,
  onDone,
}: {
  node: NodeAgent | undefined;
  onClose: () => void;
  onDone: (node: NodeAgent) => void;
}) {
  const { t } = useTranslation('infrastructure');
  const [minutes, setMinutes] = React.useState('10');
  React.useEffect(() => setMinutes('10'), [node]);

  const drain = useMutation({
    mutationFn: () => nodeAgentsApi.drainNode(node?.id ?? '', Number(minutes) * 60),
    onSuccess: (updated) => {
      toast.success(t('node_agents.drain.done'));
      onDone(updated);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  return (
    <FormDrawer
      open={Boolean(node)}
      onOpenChange={(open) => !open && onClose()}
      title={t('node_agents.drain.title', { name: node ? nodeName(node) : '' })}
      subtitle={t('node_agents.drain.subtitle')}
      width={520}
      footer={
        <FormSubmitFooter
          busy={drain.isPending}
          onCancel={onClose}
          formId="drain-node"
          submitLabel={t('node_agents.actions.drain')}
        />
      }
    >
      <form
        id="drain-node"
        onSubmit={(event) => {
          event.preventDefault();
          drain.mutate();
        }}
      >
        <FormSection>
          <FormField label={t('node_agents.drain.deadline')} hint={t('node_agents.drain.deadline_hint')}>
            <FormSelect
              value={minutes}
              onChange={setMinutes}
              options={DRAIN_MINUTES.map((value) => ({
                value: String(value),
                label: durationLabel(t, value),
              }))}
            />
          </FormField>
        </FormSection>
      </form>
    </FormDrawer>
  );
}

/** Revoke a node: its certificate stops working and its tasks end. Only enrolling again brings it back. */
export function RevokeDrawer({
  node,
  onClose,
  onDone,
}: {
  node: NodeAgent | undefined;
  onClose: () => void;
  onDone: (node: NodeAgent) => void;
}) {
  const { t } = useTranslation('infrastructure');
  const [reason, setReason] = React.useState('');
  React.useEffect(() => setReason(''), [node]);

  const revoke = useMutation({
    mutationFn: () => nodeAgentsApi.revokeNode(node?.id ?? '', reason.trim()),
    onSuccess: (updated) => {
      toast.success(t('node_agents.revoke.done'));
      onDone(updated);
    },
    onError: (error) => toast.error(toApiError(error).message),
  });

  return (
    <FormDrawer
      open={Boolean(node)}
      onOpenChange={(open) => !open && onClose()}
      title={t('node_agents.revoke.title', { name: node ? nodeName(node) : '' })}
      subtitle={t('node_agents.revoke.subtitle')}
      width={520}
      dirty={reason.trim().length > 0}
      footer={
        <FormSubmitFooter
          busy={revoke.isPending}
          invalid={!reason.trim()}
          onCancel={onClose}
          formId="revoke-node"
          submitLabel={t('node_agents.actions.revoke')}
        />
      }
    >
      <form
        id="revoke-node"
        onSubmit={(event) => {
          event.preventDefault();
          if (reason.trim()) revoke.mutate();
        }}
      >
        <FormSection>
          <FormField label={t('node_agents.revoke.reason')} required>
            <FormTextarea
              value={reason}
              maxLength={512}
              rows={3}
              onChange={(event) => setReason(event.target.value)}
              placeholder={t('node_agents.revoke.reason_placeholder')}
            />
          </FormField>
        </FormSection>
      </form>
    </FormDrawer>
  );
}

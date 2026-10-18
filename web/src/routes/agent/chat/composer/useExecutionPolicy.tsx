import type { TFunction } from 'i18next';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { ConfirmDialog } from '@/admin';
import type { AgentProfile } from '@/api/agent';

import { AUTO } from './controlCommands';
import type { ExecutionPolicy } from '../workspace/types';

export interface ChatExecutionPolicy {
  policy: ExecutionPolicy;
  /** Picks a mode; Bypass permissions is asked about first, and only where it is allowed. */
  choose: (policy: ExecutionPolicy) => void;
  /** The chat's Agent Profile lets it pick Bypass permissions. */
  bypassAllowed: boolean;
  /** The question asked before Bypass permissions is on. */
  dialog: React.ReactNode;
}

/**
 * What Mole Agent may run in the chat. Bypass permissions is there only when the chat's
 * Agent Profile allows it, and is asked about before it is on; when the profile stops
 * allowing it, the chat goes back to asking before each change.
 */
export function useExecutionPolicy({
  t,
  agentProfiles,
  agentProfileId,
}: {
  t: TFunction<'agent'>;
  agentProfiles: readonly AgentProfile[];
  /** The profile picked for the chat, or `auto` for the default one. */
  agentProfileId: string;
}): ChatExecutionPolicy {
  const { t: tc } = useTranslation('common');
  const [policy, setPolicy] = React.useState<ExecutionPolicy>('advice_only');
  const [asking, setAsking] = React.useState(false);
  const profile =
    agentProfileId !== AUTO
      ? agentProfiles.find((item) => item.id === agentProfileId)
      : agentProfiles.find((item) => item.is_default);
  const bypassAllowed = Boolean(profile?.allow_bypass_permissions);

  React.useEffect(() => {
    if (policy === 'bypass_permissions' && !bypassAllowed) setPolicy('policy');
  }, [policy, bypassAllowed]);

  const choose = React.useCallback(
    (next: ExecutionPolicy) => {
      if (next !== 'bypass_permissions') {
        setPolicy(next);
        return;
      }
      if (bypassAllowed) setAsking(true);
    },
    [bypassAllowed],
  );

  const dialog = (
    <ConfirmDialog
      open={asking}
      onOpenChange={setAsking}
      destructive
      title={t('execution_mode.bypass_confirm.title')}
      description={t('execution_mode.bypass_confirm.description')}
      cancelLabel={tc('actions.cancel')}
      confirmLabel={t('execution_mode.bypass_confirm.confirm')}
      onConfirm={() => {
        setPolicy('bypass_permissions');
        setAsking(false);
      }}
    />
  );

  return { policy, choose, bypassAllowed, dialog };
}

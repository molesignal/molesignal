import { useQueryClient } from '@tanstack/react-query';
import * as React from 'react';

import * as promptsApi from '@/api/agent/prompts';
import type { AgentPrompt } from '@/api/agent/prompts';
import * as qualityApi from '@/api/agent/quality';

import { PROMPT_GATE_KEY, PromotionDialog } from './PromotionDialog';

/**
 * Setting a prompt as the default. A personal prompt is set at once; an
 * organization prompt asks the organization's gate first, and opens the
 * check when the gate does not let it through.
 */
export function usePromotion(onSet: (prompt: AgentPrompt) => Promise<void> | void) {
  const queryClient = useQueryClient();
  const [pending, setPending] = React.useState<AgentPrompt | null>(null);
  const [busyId, setBusyId] = React.useState<string | null>(null);

  const setAsDefault = React.useCallback(
    async (prompt: AgentPrompt, confirm: boolean) => {
      await promptsApi.setDefault(prompt.id, confirm);
      setPending(null);
      await queryClient.invalidateQueries({ queryKey: [...PROMPT_GATE_KEY, prompt.id] });
      await onSet(prompt);
    },
    [onSet, queryClient],
  );

  /** Set it as the default now, or open the check first. Throws what the server answered. */
  const request = React.useCallback(
    async (prompt: AgentPrompt) => {
      setBusyId(prompt.id);
      try {
        if (prompt.scope !== 'org') {
          await setAsDefault(prompt, false);
          return;
        }
        const gate = await queryClient.fetchQuery({
          queryKey: [...PROMPT_GATE_KEY, prompt.id],
          queryFn: () => qualityApi.promptGate(prompt.id),
          staleTime: 0,
        });
        if (gate.decision === 'allow') await setAsDefault(prompt, false);
        else setPending(prompt);
      } finally {
        setBusyId(null);
      }
    },
    [queryClient, setAsDefault],
  );

  const dialog = pending ? (
    <PromotionDialog
      prompt={pending}
      onClose={() => setPending(null)}
      onSet={(confirm) => setAsDefault(pending, confirm)}
    />
  ) : null;

  return { request, dialog, busyId };
}

import { KeyRound } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { putMyMcpCredential } from '@/api/agent/mcpServers';
import type { PendingAuthorization, RespondInput } from '@/api/agent/runs';
import { toApiError } from '@/lib/http';
import { ChromeButton } from '@/shell/chrome';
import { toast } from '@/shell/ui/sonner';

/**
 * Authorize to continue: a tool of an MCP server that takes every member's own
 * credential waits for the person who asked to save theirs. Saving it here is
 * saving it in the server's settings; the call then runs again. Skipping
 * leaves the server out of this answer. The credential is theirs, so everyone
 * else in a team chat sees whose turn it is.
 */
export function AuthorizationCard({
  pending,
  canAuthorize,
  initiatorName,
  toolLabel,
  onRespond,
}: {
  pending: PendingAuthorization;
  canAuthorize: boolean;
  initiatorName: string;
  toolLabel: string;
  onRespond: (input: RespondInput) => Promise<void>;
}) {
  const { t } = useTranslation('agent');
  const [value, setValue] = React.useState('');
  const [busy, setBusy] = React.useState(false);
  const ready = value.trim().length > 0;

  const answer = async (authorized: boolean) => {
    setBusy(true);
    try {
      if (authorized) await putMyMcpCredential(pending.serverId, value);
      await onRespond({ callId: pending.callId, authorized });
      if (authorized) toast.success(t('run.authorization.saved'));
    } catch (error) {
      toast.error(toApiError(error).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div
      data-agent-authorization={pending.callId}
      className="ml-10 rounded-lg bg-[var(--purple-tint)] px-3 py-3"
    >
      <div className="flex items-start gap-2">
        <KeyRound aria-hidden="true" className="mt-0.5 h-4 w-4 shrink-0 text-[var(--purple-ink)]" />
        <div className="min-w-0 flex-1">
          <p className="text-xs font-strong text-[var(--purple-ink)]">
            {t('run.authorization.title')}
          </p>
          <p className="mt-1 text-sm text-tx-0">
            {t(pending.rejected ? 'run.authorization.rejected' : 'run.authorization.needed', {
              server: pending.server,
              tool: toolLabel,
            })}
          </p>
          {pending.instructions && (
            // Plain words, not a filled box: on the card only the field has a fill.
            <p
              data-agent-authorization-instructions
              className="mt-1 whitespace-pre-line text-xs leading-5 text-tx-2"
            >
              {pending.instructions}
            </p>
          )}
          {canAuthorize ? (
            <form
              className="mt-2"
              onSubmit={(event) => {
                event.preventDefault();
                if (ready && !busy) void answer(true);
              }}
            >
              <div className="flex flex-wrap items-center gap-2">
                <input
                  type="password"
                  autoComplete="new-password"
                  aria-label={t('run.authorization.value', { server: pending.server })}
                  placeholder={t('run.authorization.placeholder')}
                  value={value}
                  disabled={busy}
                  onChange={(event) => setValue(event.target.value)}
                  className="h-8 min-w-0 flex-1 basis-48 rounded-md bg-bg-1 px-2.5 text-xs text-tx-0 placeholder:text-tx-3"
                />
                <ChromeButton type="submit" size="sm" variant="primary" disabled={!ready || busy}>
                  {t('run.authorization.continue')}
                </ChromeButton>
                <ChromeButton size="sm" disabled={busy} onClick={() => void answer(false)}>
                  {t('run.authorization.skip')}
                </ChromeButton>
              </div>
              <p className="mt-1.5 text-xs text-tx-3">
                {t(pending.bearer ? 'run.authorization.hint_bearer' : 'run.authorization.hint', {
                  header: pending.header,
                })}
              </p>
            </form>
          ) : (
            <p className="mt-2 text-xs text-tx-2">
              {t('run.authorization.waiting', { name: initiatorName })}
            </p>
          )}
        </div>
      </div>
    </div>
  );
}

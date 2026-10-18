import type { TFunction } from 'i18next';

import type { CommandNotice } from './useChatCommands';

/** Why a command was not run or sent, over the composer that still holds it. */
export function ChatCommandNotice({
  notice,
  t,
}: {
  notice: CommandNotice | null;
  t: TFunction<'agent'>;
}) {
  if (!notice) return null;
  return (
    <div
      data-agent-command-notice={notice.code}
      role="alert"
      className="mb-2 rounded-md bg-red-dim px-3 py-2 font-sans text-xs text-[var(--red-ink)]"
    >
      {t(`commands.notice.${notice.code}`, {
        command: notice.command,
        detail: notice.detail ?? '',
      })}
    </div>
  );
}

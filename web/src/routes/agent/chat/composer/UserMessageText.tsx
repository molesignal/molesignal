import type { MessageCommand } from '@/api/agent/commands';

import { displayText, parseInput } from './commands';
import { MarkdownMessage } from '../../markdown';

/**
 * What a user message says. A command is shown as its name in a capsule with
 * the argument after it: from what the backend kept with the message, or for a
 * question not yet answered, from the text itself.
 */
export function UserMessageText({
  content,
  command,
  pending = false,
}: {
  content: string;
  command?: MessageCommand | null | undefined;
  pending?: boolean;
}) {
  const parsed = pending ? parseInput(content) : null;
  const shown =
    command ??
    (parsed?.kind === 'command' ? { name: parsed.name, args: parsed.args } : null);

  if (!shown) return <MarkdownMessage content={displayText(content)} />;
  return (
    <div className="flex min-w-0 flex-wrap items-baseline gap-x-2 gap-y-1" data-agent-command={shown.name}>
      <span className="shrink-0 rounded-md bg-indigo/15 px-1.5 font-strong text-indigo">
        /{shown.name}
      </span>
      {shown.args && (
        <span className="min-w-0">
          <MarkdownMessage content={shown.args} />
        </span>
      )}
    </div>
  );
}

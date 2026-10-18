import { useQueryClient } from '@tanstack/react-query';
import type { TFunction } from 'i18next';
import { Brain, CheckCircle2, Circle, CircleDot, Info, ListChecks } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { deleteMemory, restoreMemory } from '@/api/agent/memory';
import type { RunNotice, TodoItem } from '@/api/agent/runs';
import { toApiError } from '@/lib/http';
import { cn } from '@/shell/lib/cn';
import { toast } from '@/shell/ui/sonner';

import { type MemoryChange, todoProgress } from './runState';

/** The task list of the answer, folded to "step 2 of 5" until opened. */
export function TodoProgress({ items }: { items: TodoItem[] }) {
  const { t } = useTranslation('agent');
  const progress = todoProgress(items);
  if (!progress) return null;
  return (
    <details data-agent-todo className="group ml-10">
      <summary className="inline-flex h-7 cursor-pointer list-none items-center gap-1.5 rounded-md bg-bg-2 px-2 text-xs font-strong text-tx-1 hover:bg-bg-3 [&::-webkit-details-marker]:hidden">
        <ListChecks aria-hidden="true" className="h-3.5 w-3.5 text-indigo" />
        {t('run.todo.progress', progress)}
      </summary>
      <ol className="mt-1.5 space-y-1 rounded-md bg-bg-2 px-2.5 py-2">
        {items.map((item, index) => {
          const Icon =
            item.status === 'completed'
              ? CheckCircle2
              : item.status === 'in_progress'
                ? CircleDot
                : Circle;
          return (
            <li key={`${index}-${item.content}`} className="flex items-start gap-1.5 text-xs">
              <Icon
                aria-hidden="true"
                className={cn(
                  'mt-0.5 h-3.5 w-3.5 shrink-0',
                  item.status === 'completed' ? 'text-green' : item.status === 'in_progress' ? 'text-indigo' : 'text-tx-3',
                )}
              />
              <span
                className={cn(
                  item.status === 'completed' ? 'text-tx-3 line-through' : 'text-tx-1',
                  item.status === 'in_progress' && 'font-strong text-tx-0',
                )}
              >
                {item.content}
              </span>
            </li>
          );
        })}
      </ol>
    </details>
  );
}

function noticeText(notice: RunNotice, t: TFunction<'agent'>) {
  switch (notice.kind) {
    case 'compacted':
      return notice.automatic
        ? t('run.notice.compacted_auto')
        : t('run.notice.compacted', { count: notice.messages });
    case 'team_mounted':
      return t('run.notice.team_mounted', { team: notice.team });
    case 'no_response':
      return t('run.notice.no_response');
    case 'memories':
      return t('run.notice.memories', { count: notice.count });
    case 'bypass_unavailable':
      return t('run.notice.bypass_unavailable');
    default:
      return null;
  }
}

/** What happened on the way that the answer itself does not say. The budget has its own notice. */
export function RunNotices({ notices }: { notices: RunNotice[] }) {
  const { t } = useTranslation('agent');
  const lines = notices
    .map((notice) => noticeText(notice, t))
    .filter((line): line is string => Boolean(line));
  if (lines.length === 0) return null;
  return (
    <ul data-agent-run-notices className="ml-10 space-y-1">
      {lines.map((line, index) => (
        <li key={`${index}-${line}`} className="flex items-start gap-1.5 text-xs text-tx-2">
          <Info aria-hidden="true" className="mt-0.5 h-3.5 w-3.5 shrink-0 text-tx-3" />
          <span>{line}</span>
        </li>
      ))}
    </ul>
  );
}

/**
 * What the answer remembered or forgot, each with a way back: undoing a memory
 * just written deletes it, undoing one forgotten brings it back.
 */
export function MemoryChanges({ changes }: { changes: MemoryChange[] }) {
  const { t } = useTranslation('agent');
  const queryClient = useQueryClient();
  const [undone, setUndone] = React.useState<ReadonlySet<string>>(new Set());
  if (changes.length === 0) return null;

  const undo = async (change: MemoryChange) => {
    try {
      if (change.action === 'remembered') await deleteMemory(change.id);
      else await restoreMemory(change.id);
      setUndone((current) => new Set([...current, change.id]));
      await queryClient.invalidateQueries({ queryKey: ['agent', 'memories'] });
    } catch (error) {
      toast.error(toApiError(error).message);
    }
  };

  return (
    <ul data-agent-memory-changes className="ml-10 flex flex-wrap gap-1.5">
      {changes.map((change) => (
        <li
          key={`${change.action}-${change.id}`}
          className="inline-flex h-7 items-center gap-1.5 rounded-md bg-bg-2 pl-2 pr-1 text-xs text-tx-1"
        >
          <Brain aria-hidden="true" className="h-3.5 w-3.5 text-indigo" />
          <span className="font-strong">
            {t(`run.memory.${change.action}`, { name: change.name })}
          </span>
          {undone.has(change.id) ? (
            <span className="px-1.5 text-tx-3">{t('run.memory.undone')}</span>
          ) : (
            <button
              type="button"
              onClick={() => void undo(change)}
              className="h-5 rounded px-1.5 font-strong text-indigo hover:bg-bg-3"
            >
              {t('run.memory.undo')}
            </button>
          )}
        </li>
      ))}
    </ul>
  );
}

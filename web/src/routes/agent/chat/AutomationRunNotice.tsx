import { useQuery } from '@tanstack/react-query';
import { Workflow } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import * as chatApi from '@/api/agent/chat';
import type { Chat } from '@/api/agent/chat';

/**
 * The chat open on the page when it is a run of an automation. Such a chat is
 * not in the chat list, so it is read on its own when it is opened from a run.
 */
export function useAutomationChat(chatId: string | null, listed: Chat[] | undefined): Chat | null {
  const inList = listed?.find((chat) => chat.id === chatId);
  const fetched = useQuery({
    queryKey: ['agent', 'chat', chatId],
    queryFn: () => chatApi.getChat(chatId as string),
    enabled: Boolean(chatId) && listed !== undefined && !inList,
    retry: false,
    staleTime: 60_000,
  });
  const chat = inList ?? fetched.data ?? null;
  return chat?.automationId ? chat : null;
}

/**
 * Said where the question would be typed: this chat is a run of an
 * automation, kept to be read. Asking again means running the automation.
 */
export function AutomationRunNotice({ chat }: { chat: Chat }) {
  const { t } = useTranslation('agent');

  return (
    <div
      data-agent-automation-run={chat.automationId ?? ''}
      role="status"
      className="flex flex-wrap items-center gap-x-3 gap-y-1 rounded-md bg-bg-2 px-3 py-2.5 font-sans text-xs text-tx-1"
    >
      <Workflow aria-hidden="true" className="h-3.5 w-3.5 shrink-0 text-tx-2" />
      <span className="min-w-0 flex-1">{t('automation_run.notice', { name: chat.title })}</span>
      <Link
        to="/agent/automations"
        className="shrink-0 rounded-sm font-strong text-tx-0 underline-offset-4 hover:underline focus-visible:underline focus-visible:outline-none"
      >
        {t('automation_run.open')}
      </Link>
    </div>
  );
}

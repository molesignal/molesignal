import {
  CircleGauge,
  MessageSquareText,
  Plus,
  Trash2,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import type { Chat } from '@/api/agent/chat';
import { cn } from '@/shell/lib/cn';

import {
  CHAT_LIST_MAX_WIDTH,
  CHAT_LIST_MIN_WIDTH,
} from '../listResize';
import { displayTitleForChat, groupChats } from '../presentation';

export interface ConversationHistoryProps {
  chats: readonly Chat[];
  selectedChatId: string | null;
  streaming: boolean;
  onNewChat: () => void;
  onSelectChat: (chat: Chat) => void;
  onDeleteChat: (chat: Chat) => void;
  variant?: 'sidebar' | 'drawer';
  width?: number;
  resizing?: boolean;
  onResizePointerDown?: (event: React.PointerEvent<HTMLDivElement>) => void;
  onResizeKeyDown?: (event: React.KeyboardEvent<HTMLDivElement>) => void;
  onResetWidth?: () => void;
}

export function ConversationHistory({
  chats,
  selectedChatId,
  streaming,
  onNewChat,
  onSelectChat,
  onDeleteChat,
  variant = 'sidebar',
  width,
  resizing = false,
  onResizePointerDown,
  onResizeKeyDown,
  onResetWidth,
}: ConversationHistoryProps) {
  const { t } = useTranslation('agent');
  const groups = groupChats(chats);
  const content = (
    <>
      <header className="flex items-center justify-between px-3 py-2.5">
        <h2 className="font-sans text-xs font-strong text-tx-1">{t('chats')}</h2>
        <button
          type="button"
          onClick={onNewChat}
          className={cn(
            'inline-flex items-center gap-1 rounded-md px-2 font-sans text-xs font-strong text-indigo transition-colors duration-fast hover:bg-indigo/10 focus-visible:bg-indigo/10',
            variant === 'drawer' ? 'min-h-11' : 'min-h-8',
          )}
        >
          <Plus aria-hidden="true" className="h-3.5 w-3.5" />
          {t('new_chat')}
        </button>
      </header>

      <div className="min-h-0 flex-1 overflow-auto px-2 pb-3">
        {groups.length === 0 ? (
          <div className="flex min-h-44 flex-col items-center justify-center px-4 text-center">
            <MessageSquareText aria-hidden="true" className="h-5 w-5 text-tx-4" />
            <p className="mt-3 type-caption font-strong text-tx-1">
              {t('history_empty_title')}
            </p>
            <p className="mt-1 max-w-44 type-micro leading-5 text-tx-3">
              {t('history_empty_description')}
            </p>
          </div>
        ) : (
          groups.map((group) => (
            <section key={group.key} className="pt-3">
              <h3 className="px-2 pb-1.5 font-sans text-type-micro font-strong uppercase tracking-[0.08em] text-tx-3">
                {t(`chat_groups.${group.key}`)}
              </h3>
              <div className="space-y-1">
                {group.chats.map((chat) => {
                  const active = chat.id === selectedChatId;
                  const investigating = active && streaming;
                  const isInvestigation = Boolean(chat.analysis_mode);
                  const ChatIcon = isInvestigation
                    ? CircleGauge
                    : MessageSquareText;

                  return (
                    <div
                      key={chat.id}
                      className={cn(
                        'group/chat relative flex items-center rounded-md transition-colors duration-fast',
                        active
                          ? 'bg-indigo/10 text-tx-0'
                          : 'text-tx-1 hover:bg-bg-3',
                      )}
                    >
                      <button
                        type="button"
                        onClick={() => onSelectChat(chat)}
                        className={cn(
                          'flex min-w-0 flex-1 items-center gap-2.5 px-2 text-left focus-visible:bg-bg-2',
                          variant === 'drawer'
                            ? 'min-h-11 py-2'
                            : 'min-h-10 py-1.5',
                        )}
                      >
                        <ChatIcon
                          aria-hidden="true"
                          className={cn(
                            'h-3.5 w-3.5 shrink-0 text-tx-3',
                            investigating && 'text-indigo',
                          )}
                        />
                        <span className="flex min-w-0 flex-1 items-baseline gap-2">
                          <span
                            data-testid="conversation-history-title"
                            className={cn(
                              'min-w-0 flex-1 truncate font-sans font-strong',
                              variant === 'drawer' ? 'text-base' : 'text-xs',
                            )}
                          >
                            {displayTitleForChat(chat, chats) || t('untitled')}
                          </span>
                          <span
                            data-testid="conversation-history-time"
                            className="shrink-0 font-sans text-type-micro text-tx-3"
                          >
                            {messageTime(chat.updated_at_micros)}
                          </span>
                        </span>
                      </button>
                      <button
                        type="button"
                        aria-label={t('chat_delete')}
                        title={t('chat_delete')}
                        onClick={() => onDeleteChat(chat)}
                        className={cn(
                          'mr-1 grid shrink-0 place-items-center rounded-md text-tx-3 transition-colors duration-fast hover:bg-bg-1 hover:text-red focus-visible:bg-bg-1 focus-visible:text-red',
                          variant === 'drawer'
                            ? 'h-11 w-11'
                            : 'h-7 w-7 opacity-0 focus-visible:opacity-100 group-hover/chat:opacity-100',
                        )}
                      >
                        <Trash2 aria-hidden="true" className="h-3.5 w-3.5" />
                      </button>
                    </div>
                  );
                })}
              </div>
            </section>
          ))
        )}
      </div>
    </>
  );

  if (variant === 'drawer') {
    return (
      <div className="flex h-full min-h-0 flex-col bg-[var(--control-surface)]">
        {content}
      </div>
    );
  }

  return (
    <aside
      data-testid="conversation-history"
      aria-label={t('chats')}
      className="relative hidden shrink-0 flex-col rounded-md bg-[var(--control-surface)] md:flex"
      style={{ width }}
    >
      {content}
      <div
        role="separator"
        aria-orientation="vertical"
        aria-label={t('chat_list_resize')}
        aria-controls="agent-chat-transcript"
        aria-valuemin={CHAT_LIST_MIN_WIDTH}
        aria-valuemax={CHAT_LIST_MAX_WIDTH}
        aria-valuenow={width}
        aria-valuetext={`${width}px`}
        tabIndex={0}
        title={t('chat_list_resize')}
        data-testid="agent-chat-list-resizer"
        data-resizing={resizing || undefined}
        onPointerDown={onResizePointerDown}
        onKeyDown={onResizeKeyDown}
        onDoubleClick={onResetWidth}
        className="group absolute inset-y-0 -right-1 z-20 w-2 touch-none cursor-col-resize select-none focus-visible:outline-none"
      >
        <span
          aria-hidden="true"
          className={cn(
            'absolute inset-y-0 left-1/2 w-px -translate-x-1/2 bg-transparent transition-colors duration-fast',
            'group-hover:bg-indigo group-focus-visible:bg-indigo',
            resizing && 'bg-indigo',
          )}
        />
      </div>
    </aside>
  );
}

function messageTime(micros: number): string {
  const value = new Date(micros / 1000);
  if (Number.isNaN(value.getTime())) return '';
  return value.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
}

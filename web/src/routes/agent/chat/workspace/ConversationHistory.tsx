import { useQueryClient } from '@tanstack/react-query';
import {
  CircleGauge,
  MessageSquareText,
  Pencil,
  Plus,
  Trash2,
  Users,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';

import { type Chat, renameChat } from '@/api/agent/chat';
import { toApiError } from '@/lib/http';
import { cn } from '@/shell/lib/cn';
import { toast } from '@/shell/ui/sonner';

import {
  CHAT_LIST_MAX_WIDTH,
  CHAT_LIST_MIN_WIDTH,
} from '../listResize';
import { displayTitleForChat } from '../presentation';
import { arrangeChats } from './chatTree';

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
  /** The name of a team, for the team chats in the list. */
  teamName?: (id: string | null | undefined) => string | null;
}

type ChatFilter = 'all' | 'personal' | 'team';

const FILTERS: readonly ChatFilter[] = ['all', 'personal', 'team'];

function inFilter(chat: Chat, filter: ChatFilter): boolean {
  if (filter === 'all') return true;
  return (chat.scope === 'team') === (filter === 'team');
}

/** Where the list's chats are kept, as the chat page reads them. */
const CHATS_KEY = ['agent', 'chats'] as const;

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
  teamName,
}: ConversationHistoryProps) {
  const { t } = useTranslation('agent');
  const queryClient = useQueryClient();
  const [filter, setFilter] = React.useState<ChatFilter>('all');
  const [renaming, setRenaming] = React.useState<{ id: string; title: string } | null>(null);
  // The filter is offered only once there are team chats to tell apart.
  const hasTeamChats = chats.some((chat) => chat.scope === 'team');
  const shown = hasTeamChats ? chats.filter((chat) => inFilter(chat, filter)) : chats;
  const groups = arrangeChats(shown);
  const drawer = variant === 'drawer';

  // A new name is kept at once; memories name the chat they came from by it.
  const rename = async (chat: Chat, title: string) => {
    setRenaming(null);
    const next = title.trim();
    if (!next || next === chat.title) return;
    try {
      const renamed = await renameChat(chat.id, next);
      queryClient.setQueryData<Chat[]>(CHATS_KEY, (current) =>
        current?.map((item) => (item.id === renamed.id ? { ...item, title: renamed.title } : item)),
      );
      void queryClient.invalidateQueries({ queryKey: ['agent', 'memories'] });
    } catch (error) {
      toast.error(toApiError(error).message);
    }
  };

  const newChatButton = (
    <button
      type="button"
      onClick={onNewChat}
      aria-label={t('new_chat')}
      title={t('new_chat')}
      className={cn(
        'grid shrink-0 place-items-center rounded-md text-indigo transition-colors duration-fast hover:bg-indigo/10 focus-visible:bg-indigo/10',
        drawer ? 'h-11 w-11' : 'h-7 w-7',
      )}
    >
      <Plus aria-hidden="true" className="h-4 w-4" />
    </button>
  );

  const content = (
    <>
      {/* The list needs no title of its own: the tab names it. */}
      <h2 className="sr-only">{t('chats')}</h2>

      {hasTeamChats ? (
        <div className="flex items-center gap-1 px-3 py-[4px]">
          <div
            role="group"
            aria-label={t('chat_filter.label')}
            data-testid="conversation-history-filter"
            className="flex min-w-0 flex-1 gap-1"
          >
            {FILTERS.map((value) => (
              <button
                key={value}
                type="button"
                aria-pressed={filter === value}
                onClick={() => setFilter(value)}
                className={cn(
                  'inline-flex h-7 items-center gap-1 rounded-md px-2 font-sans type-micro font-strong transition-colors duration-fast',
                  filter === value
                    ? 'bg-bg-3 text-tx-0'
                    : 'text-tx-3 hover:bg-bg-2 hover:text-tx-1',
                )}
              >
                {t(`chat_filter.${value}`)}
                <span className="text-tx-3">
                  {chats.filter((chat) => inFilter(chat, value)).length}
                </span>
              </button>
            ))}
          </div>
          {newChatButton}
        </div>
      ) : (
        // No row of its own: the way to a new chat sits on the line of the day the list is at.
        <div
          data-testid="conversation-history-new"
          className={cn('absolute right-2 z-20', drawer ? 'top-0.5' : 'top-1')}
        >
          {newChatButton}
        </div>
      )}

      <div className="min-h-0 flex-1 overflow-auto px-2 pb-3">
        {groups.length === 0 ? (
          <div
            className={cn(
              'flex min-h-44 flex-col items-center justify-center px-4 text-center',
              !hasTeamChats && 'pt-9',
            )}
          >
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
            <section key={group.key}>
              {/* The day stays on top while its chats scroll under it. */}
              <h3
                className={cn(
                  'sticky top-0 z-10 flex items-center bg-[var(--control-surface)] px-2 font-sans type-micro font-strong tracking-normal text-tx-3',
                  drawer ? 'h-12' : 'h-9',
                  !hasTeamChats && (drawer ? 'pr-14' : 'pr-10'),
                )}
              >
                {t(`chat_groups.${group.key}`)}
              </h3>
              <div className="space-y-1">
                {group.rows.map(({ chat, depth }) => {
                  const active = chat.id === selectedChatId;
                  const investigating = active && streaming;
                  const isInvestigation = Boolean(chat.analysis_mode);
                  const ChatIcon = isInvestigation
                    ? CircleGauge
                    : MessageSquareText;
                  const title = displayTitleForChat(chat, chats) || t('untitled');

                  return (
                    <div
                      key={chat.id}
                      data-chat-depth={depth}
                      className={cn(
                        'group/chat relative flex items-center rounded-md transition-colors duration-fast',
                        active
                          ? 'bg-indigo/10 text-tx-0'
                          : 'text-tx-1 hover:bg-bg-3',
                      )}
                    >
                      {renaming?.id === chat.id ? (
                        <input
                          // A name is written where it is read: Enter keeps it, Escape leaves it.
                          autoFocus
                          aria-label={t('chat_rename_label')}
                          data-testid="conversation-history-rename"
                          value={renaming.title}
                          maxLength={255}
                          onFocus={(event) => event.currentTarget.select()}
                          onChange={(event) =>
                            setRenaming({ id: chat.id, title: event.target.value })
                          }
                          onKeyDown={(event) => {
                            if (event.key === 'Enter') {
                              event.preventDefault();
                              void rename(chat, renaming.title);
                            } else if (event.key === 'Escape') {
                              event.preventDefault();
                              setRenaming(null);
                            }
                          }}
                          onBlur={() => void rename(chat, renaming.title)}
                          className={cn(
                            'mx-1 min-w-0 flex-1 rounded-md bg-[var(--functional-surface)] px-2 font-sans font-strong text-tx-0 outline-none',
                            drawer ? 'h-11 text-base' : 'my-1 h-8 text-xs',
                          )}
                        />
                      ) : (
                        <button
                          type="button"
                          onClick={() => onSelectChat(chat)}
                          className={cn(
                            'flex min-w-0 flex-1 items-center gap-2.5 px-2 text-left focus-visible:bg-bg-2',
                            drawer ? 'min-h-11 py-2' : 'min-h-10 py-1.5',
                          )}
                        >
                          {depth > 0 ? (
                            // A fork hangs under the chat it came from, on a corner line.
                            <span
                              aria-hidden="true"
                              style={{ marginLeft: (depth - 1) * 14 }}
                              className="mb-1.5 h-2.5 w-3 shrink-0 self-center rounded-bl-[4px] border-b border-l border-bd-2"
                            />
                          ) : (
                            <ChatIcon
                              aria-hidden="true"
                              className={cn(
                                'h-3.5 w-3.5 shrink-0 text-tx-3',
                                investigating && 'text-indigo',
                              )}
                            />
                          )}
                          <span className="flex min-w-0 flex-1 items-baseline gap-2">
                            <span
                              data-testid="conversation-history-title"
                              className={cn(
                                'min-w-0 flex-1 truncate font-sans font-strong',
                                drawer ? 'text-base' : 'text-xs',
                              )}
                            >
                              {title}
                            </span>
                            {chat.scope === 'team' && (
                              <span
                                data-testid="conversation-history-team"
                                className="inline-flex max-w-24 shrink-0 items-center gap-1 font-sans type-micro text-tx-3"
                              >
                                <Users aria-hidden="true" className="h-3 w-3 shrink-0" />
                                <span className="truncate">
                                  {teamName?.(chat.team_id) ?? t('team.label')}
                                </span>
                              </span>
                            )}
                          </span>
                        </button>
                      )}
                      {renaming?.id !== chat.id && (
                        <span
                          className={cn(
                            'mr-1 flex shrink-0 items-center',
                            !drawer &&
                              'opacity-0 focus-within:opacity-100 group-hover/chat:opacity-100',
                          )}
                        >
                          <RowAction
                            label={t('chat_rename')}
                            variant={variant}
                            onClick={() => setRenaming({ id: chat.id, title: chat.title })}
                          >
                            <Pencil aria-hidden="true" className="h-3.5 w-3.5" />
                          </RowAction>
                          <RowAction
                            label={t('chat_delete')}
                            variant={variant}
                            danger
                            onClick={() => onDeleteChat(chat)}
                          >
                            <Trash2 aria-hidden="true" className="h-3.5 w-3.5" />
                          </RowAction>
                        </span>
                      )}
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

  if (drawer) {
    return (
      <div className="relative flex h-full min-h-0 flex-col bg-[var(--control-surface)]">
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

/** One action on a chat in the list: shown while the row is pointed at, always in the drawer. */
function RowAction({
  label,
  variant,
  danger = false,
  onClick,
  children,
}: {
  label: string;
  variant: 'sidebar' | 'drawer';
  danger?: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
      className={cn(
        'grid shrink-0 place-items-center rounded-md text-tx-3 transition-colors duration-fast hover:bg-bg-1 focus-visible:bg-bg-1',
        danger ? 'hover:text-red focus-visible:text-red' : 'hover:text-tx-0 focus-visible:text-tx-0',
        variant === 'drawer' ? 'h-11 w-11' : 'h-7 w-7',
      )}
    >
      {children}
    </button>
  );
}

import { useQuery } from '@tanstack/react-query';
import { MessageSquareText, SlidersHorizontal } from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link, useSearchParams } from 'react-router-dom';

import { ConfirmDialog } from '@/admin';
import * as agentApi from '@/api/agent';
import * as chatApi from '@/api/agent/chat';
import * as providersApi from '@/api/agent/modelProviders';
import * as promptsApi from '@/api/agent/prompts';
import { compactChat } from '@/api/agent/runs';
import * as meApi from '@/api/me';
import {
  AGENT_CONTEXT_FILTER_KEYS,
  chatContextFromFilters,
  contextStreamHints,
  timePresetFromWindow,
  timeRangeMicros,
  timeWindowForPreset,
} from '@/investigation/agentContext';
import { toApiError } from '@/lib/http';
import { useActionAccess } from '@/product/actionAccess';
import { ProductState } from '@/product/states';
import { cn } from '@/shell/lib/cn';
import {
  Sheet,
  SheetContent,
  SheetTitle,
} from '@/shell/ui/sheet';
import { toast } from '@/shell/ui/sonner';
import { useUsers } from '@/shell/useUsers';
import { useAuthStore } from '@/stores/auth';
import { useFiltersStore } from '@/stores/useFiltersStore';
import { useMoleAgentStore } from '@/stores/useMoleAgentStore';
import { useTimeStore } from '@/stores/useTimeStore';
import { formatTimeWindowLabel } from '@/time/TimePicker';

import { useFeedbackMap } from '../feedback/useFeedback';
import {
  conversationItems,
  formatStreamErrorContent,
  hasPersistedPendingUser,
  type PendingUserMessage,
  visibleChatMessages,
} from '../messageRenderState';
import { ChatBudgetNotice } from './BudgetNotice';
import { ChatComposer } from './composer/ChatComposer';
import { ChatCommandNotice } from './composer/CommandNotice';
import { parseInput } from './composer/commands';
import type { ComposerHandle } from './composer/ComposerEditor';
import { AUTO, PERSONAL } from './composer/controlCommands';
import { chatMarkdown, downloadText, exportFileName, exportLabels } from './composer/exportChat';
import { useChatCommands } from './composer/useChatCommands';
import { useExecutionPolicy } from './composer/useExecutionPolicy';
import {
  CHAT_LIST_DEFAULT_WIDTH,
  CHAT_LIST_WIDTH_STORAGE_KEY,
  chatListWidthFromKey,
  chatListWidthFromPointer,
  clampChatListWidth,
  parseStoredChatListWidth,
} from './listResize';
import { displayTitleForChat, titleForNewChat } from './presentation';
import { LiveRun } from './run/LiveRun';
import { useChatRun } from './run/useChatRun';
import { useChatTeam } from './team/useChatTeam';
import { ConversationTurnView } from './transcript/ConversationTurn';
import { type ChatUserIdentity, MessageBubble } from './transcript/MessageBubble';
import { PinnedAnswers } from './transcript/PinnedAnswers';
import { useMessagePins } from './transcript/useMessagePins';
import { AgentWorkspace } from './workspace/AgentWorkspace';
import { ConversationHistory } from './workspace/ConversationHistory';
import { SystemContext } from './workspace/SystemContext';
import { type ChatContext, type ChatMode, type StarterSelection } from './workspace/types';

export { aggregateTools, summarizeToolPayload } from './transcript/toolSummary';

const STICKY_SCROLL_THRESHOLD_PX = 12;


function analysisModeForChatMode(mode: ChatMode): string | undefined {
  if (mode === 'quick') return 'anomaly_analysis';
  if (mode === 'deep') return 'root_cause';
  if (mode === 'query_only') return 'query_generation';
  return undefined;
}

function purposeForChatMode(mode: ChatMode): promptsApi.PromptPurpose | null {
  const purpose = analysisModeForChatMode(mode);
  return purpose ? (purpose as promptsApi.PromptPurpose) : null;
}

/** What a new chat is named after: a command's argument rather than the command. */
function titleSource(content: string): string {
  const parsed = parseInput(content);
  if (parsed.kind === 'text') return parsed.text;
  return parsed.args || `/${parsed.name}`;
}

function chatModeForAnalysisMode(mode: string | null | undefined): ChatMode {
  if (mode === 'anomaly_analysis' || mode === 'alert_explain') return 'quick';
  if (mode === 'root_cause') return 'deep';
  if (mode === 'query_generation') return 'query_only';
  return 'auto';
}

export function isNearScrollBottom(
  el: Pick<HTMLDivElement, 'scrollHeight' | 'scrollTop' | 'clientHeight'>,
  thresholdPx = STICKY_SCROLL_THRESHOLD_PX,
): boolean {
  return el.scrollHeight - el.scrollTop - el.clientHeight <= thresholdPx;
}

export function shouldPauseAutoScrollForWheel(deltaY: number): boolean {
  return deltaY < 0;
}

const SCROLL_FADE_EDGE_PX = 4;

/**
 * Whether the transcript is parked against its top / bottom edge. When a side
 * is at the edge its scroll-fade is retracted so a pinned first / last message
 * isn't dimmed with nothing more to scroll toward. A non-overflowing transcript
 * reports both true → no fade at all.
 */
export function scrollFadeState(
  el: Pick<HTMLDivElement, 'scrollHeight' | 'scrollTop' | 'clientHeight'>,
): { top: boolean; bottom: boolean } {
  return {
    top: el.scrollTop <= SCROLL_FADE_EDGE_PX,
    bottom: el.scrollHeight - el.scrollTop - el.clientHeight <= SCROLL_FADE_EDGE_PX,
  };
}

// Drives the mask edges imperatively — no React state, so scrolling doesn't
// re-render the tree. Empty string clears the inline value, falling back to the
// default fade size declared in CSS.
function applyScrollFade(el: HTMLDivElement): void {
  const { top, bottom } = scrollFadeState(el);
  el.style.setProperty('--fade-top', top ? '0px' : '');
  el.style.setProperty('--fade-bottom', bottom ? '0px' : '');
}

export function AgentChat({ embedded = false }: { embedded?: boolean } = {}) {
  const { t } = useTranslation('agent');
  const { t: tc } = useTranslation('common');
  const manageAccess = useActionAccess({ permission: 'agent.manage' });
  const authCtx = useAuthStore((s) => s.ctx);
  const globalTimeWindow = useTimeStore((state) => state.window);
  const setGlobalTimeWindow = useTimeStore((state) => state.setWindow);
  const globalFilters = useFiltersStore((state) => state.filters);
  const context = React.useMemo(
    () => chatContextFromFilters(globalFilters),
    [globalFilters],
  );
  const rangePreset = timePresetFromWindow(globalTimeWindow);
  // Under the name the time picker gives it, in the product's language: "Last 24h" stood here
  // in English on a page written in Chinese.
  const rangeLabel = formatTimeWindowLabel(globalTimeWindow, tc);
  const setRangePreset = React.useCallback(
    (preset: string) => {
      const next = timeWindowForPreset(preset);
      if (next) setGlobalTimeWindow(next);
    },
    [setGlobalTimeWindow],
  );
  const setContext = React.useCallback<
    React.Dispatch<React.SetStateAction<ChatContext>>
  >((update) => {
    const store = useFiltersStore.getState();
    const current = chatContextFromFilters(store.filters);
    const next = typeof update === 'function' ? update(current) : update;
    for (const field of ['environment', 'service', 'alert'] as const) {
      for (const alias of AGENT_CONTEXT_FILTER_KEYS[field]) {
        store.removeFilter(alias);
      }
      if (next[field].trim()) store.setFilter(field, next[field].trim());
    }
  }, []);
  const chatsQ = useQuery({
    queryKey: ['agent', 'chats'],
    queryFn: () => chatApi.listChats(),
    retry: false,
  });
  const [chatId, setChatId] = React.useState<string | null>(null);
  // A chat not started yet is personal unless a team is chosen; a started one keeps its team.
  const chatTeam = useChatTeam((chatsQ.data ?? []).find((chat) => chat.id === chatId));
  const chatTeamRef = React.useRef(chatTeam);
  chatTeamRef.current = chatTeam;
  const messagesQ = useQuery({
    queryKey: ['chat-messages', chatId],
    queryFn: () => chatApi.listMessages(chatId as string),
    enabled: !!chatId,
    retry: false,
  });
  const providersQ = useQuery({
    queryKey: ['agent', 'model-providers'],
    queryFn: () => providersApi.list(),
    retry: false,
  });
  const promptsQ = useQuery({
    queryKey: ['agent', 'prompts'],
    queryFn: () => promptsApi.list(),
    retry: false,
  });
  const agentProfilesQ = useQuery({
    queryKey: ['agent', 'agent-profiles'],
    queryFn: () => agentApi.listProfiles(),
    retry: false,
  });
  const profileQ = useQuery({
    queryKey: ['me', 'profile'],
    queryFn: () => meApi.profile(),
    enabled: !!authCtx,
    retry: false,
  });

  const [input, setInput] = React.useState('');
  const [mode, setMode] = React.useState<ChatMode>('auto');
  const [capability, setCapability] =
    React.useState<chatApi.ChatCapability | null>(null);
  const [agentProfileId, setAgentProfileId] = React.useState(AUTO);
  const [providerId, setProviderId] = React.useState(AUTO);
  const [promptId, setPromptId] = React.useState(AUTO);
  const [chatListWidth, setChatListWidth] = React.useState(() => {
    if (typeof window === 'undefined') return CHAT_LIST_DEFAULT_WIDTH;
    try {
      return parseStoredChatListWidth(
        window.localStorage.getItem(CHAT_LIST_WIDTH_STORAGE_KEY),
      );
    } catch {
      return CHAT_LIST_DEFAULT_WIDTH;
    }
  });
  const [isChatListResizing, setIsChatListResizing] = React.useState(false);
  const [mobileHistoryOpen, setMobileHistoryOpen] = React.useState(false);
  const [mobileContextOpen, setMobileContextOpen] = React.useState(false);
  const [chatPendingDeletion, setChatPendingDeletion] =
    React.useState<chatApi.Chat | null>(null);
  const [deletingChat, setDeletingChat] = React.useState(false);

  const [pendingUser, setPendingUser] = React.useState<PendingUserMessage | null>(null);
  // A message that could not be queued; an answer that failed is kept in the chat instead.
  const [streamError, setStreamError] = React.useState<string | null>(null);
  const [budgetNotice, setBudgetNotice] = React.useState<chatApi.BudgetNotice | null>(null);
  const [regeneratingFromMessageId, setRegeneratingFromMessageId] =
    React.useState<string | null>(null);
  // What the next question carries: things attached with @, and for a new chat what it investigates.
  const [references, setReferences] = React.useState<chatApi.MessageReference[]>([]);
  const [subject, setSubject] = React.useState<chatApi.CreateChatInput['subject']>(undefined);
  // The answers are runs: they go on when the page is left and are read on when it comes back.
  const chatRun = useChatRun(
    chatId,
    async () => {
      await messagesQ.refetch();
      void chatsQ.refetch();
    },
    // The budget said along an answer stays over the composer until the next question.
    (notice) => notice.kind === 'budget' && setBudgetNotice({ ...notice, blocked: false }),
  );
  const streaming = chatRun.busy;
  const users = useUsers();
  const [searchParams] = useSearchParams();
  const scrollRef = React.useRef<HTMLDivElement | null>(null);
  const messagePins = useMessagePins(chatId);
  const shouldStickToBottomRef = React.useRef(true);
  const lastTouchYRef = React.useRef<number | null>(null);
  const composerRef = React.useRef<ComposerHandle>(null);
  const chatListWidthRef = React.useRef(chatListWidth);
  const resizeCleanupRef = React.useRef<(() => void) | null>(null);

  // "Ask Mole Agent" entry points (e.g. the Home verdict) hand a question over
  // through the shell store. It is prefilled for the operator to read and edit,
  // never sent on their behalf.
  const moleAgentSeed = useMoleAgentStore((state) => state.seed);
  React.useEffect(() => {
    if (!moleAgentSeed) return;
    setInput(moleAgentSeed);
    useMoleAgentStore.setState({ seed: null });
    window.requestAnimationFrame(() => composerRef.current?.focus());
  }, [moleAgentSeed]);

  // A page asked for a new chat (an incident to investigate, knowledge to change): it starts
  // for the team and with what is attached, and the question waits to be read and sent.
  const moleAgentIntent = useMoleAgentStore((state) => state.intent);
  React.useEffect(() => {
    if (!moleAgentIntent) return;
    useMoleAgentStore.setState({ intent: null });
    setChatId(null);
    setPendingUser(null);
    setStreamError(null);
    chatTeamRef.current.setTeamId(moleAgentIntent.teamId ?? null);
    setSubject(moleAgentIntent.subject ?? undefined);
    setReferences(moleAgentIntent.references ?? []);
    setInput(moleAgentIntent.prompt);
    window.requestAnimationFrame(() => composerRef.current?.focus());
  }, [moleAgentIntent]);

  // `?chat=` opens a chat: the investigations list links to the chats it records.
  React.useEffect(() => {
    const wanted = searchParams.get('chat');
    if (wanted && !embedded) setChatId(wanted);
    // Read once, when the page opens.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const applyChatListWidth = React.useCallback((width: number, persist = false) => {
    const nextWidth = clampChatListWidth(width);
    chatListWidthRef.current = nextWidth;
    setChatListWidth(nextWidth);
    if (persist && typeof window !== 'undefined') {
      try {
        window.localStorage.setItem(CHAT_LIST_WIDTH_STORAGE_KEY, String(nextWidth));
      } catch {
        // Resizing still works when storage is unavailable or disabled.
      }
    }
  }, []);

  const beginChatListResize = React.useCallback(
    (event: React.PointerEvent<HTMLDivElement>) => {
      if (event.button !== 0) return;
      event.preventDefault();
      resizeCleanupRef.current?.();

      const pointerId = event.pointerId;
      const startClientX = event.clientX;
      const startWidth = chatListWidthRef.current;
      const previousCursor = document.body.style.cursor;
      const previousUserSelect = document.body.style.userSelect;

      const cleanup = () => {
        globalThis.removeEventListener('pointermove', update);
        globalThis.removeEventListener('pointerup', finish);
        globalThis.removeEventListener('pointercancel', finish);
        document.body.style.cursor = previousCursor;
        document.body.style.userSelect = previousUserSelect;
        resizeCleanupRef.current = null;
      };
      const update = (pointerEvent: PointerEvent) => {
        if (pointerEvent.pointerId !== pointerId) return;
        pointerEvent.preventDefault();
        applyChatListWidth(
          chatListWidthFromPointer(startWidth, startClientX, pointerEvent.clientX),
        );
      };
      const finish = (pointerEvent: PointerEvent) => {
        if (pointerEvent.pointerId !== pointerId) return;
        cleanup();
        setIsChatListResizing(false);
        applyChatListWidth(chatListWidthRef.current, true);
      };

      resizeCleanupRef.current = cleanup;
      document.body.style.cursor = 'col-resize';
      document.body.style.userSelect = 'none';
      setIsChatListResizing(true);
      globalThis.addEventListener('pointermove', update, { passive: false });
      globalThis.addEventListener('pointerup', finish);
      globalThis.addEventListener('pointercancel', finish);
      try {
        event.currentTarget.setPointerCapture(pointerId);
      } catch {
        // Window-level pointer tracking keeps the interaction alive.
      }
    },
    [applyChatListWidth],
  );

  const handleChatListResizeKeyDown = React.useCallback(
    (event: React.KeyboardEvent<HTMLDivElement>) => {
      const nextWidth = chatListWidthFromKey(
        chatListWidthRef.current,
        event.key,
        event.shiftKey,
      );
      if (nextWidth === null) return;
      event.preventDefault();
      applyChatListWidth(nextWidth, true);
    },
    [applyChatListWidth],
  );

  const resetChatListWidth = React.useCallback(() => {
    applyChatListWidth(CHAT_LIST_DEFAULT_WIDTH, true);
  }, [applyChatListWidth]);

  React.useEffect(
    () => () => {
      resizeCleanupRef.current?.();
    },
    [],
  );

  const handleTranscriptScroll = React.useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    shouldStickToBottomRef.current = isNearScrollBottom(el);
    applyScrollFade(el);
  }, []);

  const handleTranscriptWheel = React.useCallback((event: React.WheelEvent<HTMLDivElement>) => {
    if (shouldPauseAutoScrollForWheel(event.deltaY)) {
      shouldStickToBottomRef.current = false;
    }
  }, []);

  const handleTranscriptTouchStart = React.useCallback((event: React.TouchEvent<HTMLDivElement>) => {
    lastTouchYRef.current = event.touches[0]?.clientY ?? null;
  }, []);

  const handleTranscriptTouchMove = React.useCallback((event: React.TouchEvent<HTMLDivElement>) => {
    const currentY = event.touches[0]?.clientY ?? null;
    const lastY = lastTouchYRef.current;
    if (currentY !== null && lastY !== null && currentY > lastY) {
      shouldStickToBottomRef.current = false;
    }
    lastTouchYRef.current = currentY;
  }, []);

  React.useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    if (shouldStickToBottomRef.current) el.scrollTo({ top: el.scrollHeight });
    applyScrollFade(el);
  }, [messagesQ.data, chatRun.view, pendingUser]);

  // The signed-in person's 👍 / 👎 on the answers of this chat, in one request.
  const feedbackByAnswer = useFeedbackMap(
    'chat_message',
    (messagesQ.data ?? [])
      .filter((message) => message.role === 'assistant')
      .map((message) => message.id),
  );

  const providers = (providersQ.data ?? []).filter((p) => p.enabled);
  const prompts = promptsQ.data ?? [];
  const agentProfiles = (agentProfilesQ.data ?? []).filter((profile) => profile.enabled);
  const execution = useExecutionPolicy({ t, agentProfiles, agentProfileId });
  const executionPolicy = execution.policy;
  const selectedPurpose = purposeForChatMode(mode);
  const promptOptions = prompts.filter(
    (prompt) =>
      prompt.enabled &&
      prompt.purpose !== 'system' &&
      (!selectedPurpose || prompt.purpose === selectedPurpose),
  );

  const newChat = () => {
    shouldStickToBottomRef.current = true;
    setChatId(null);
    chatTeam.setTeamId(null);
    setPendingUser(null);
    setStreamError(null);
    setRegeneratingFromMessageId(null);
    setCapability(null);
    setReferences([]);
    setSubject(undefined);
    setMobileHistoryOpen(false);
  };

  // `/compact`: the chat so far becomes a summary, as a run of the chat.
  const compactCurrentChat = () => {
    if (!chatId || (messagesQ.data ?? []).length === 0) return false;
    compactChat(chatId)
      .then((run) => chatRun.follow(run))
      .catch((error: unknown) => toast.error(toApiError(error).message));
    return true;
  };

  // `/export`: the chat on screen as a Markdown file.
  const exportChat = () => {
    const rows = messagesQ.data ?? [];
    if (!chatId || rows.length === 0) return false;
    const current = (chatsQ.data ?? []).find((chat) => chat.id === chatId);
    const title =
      (current && displayTitleForChat(current, chatsQ.data ?? [])) || t('untitled');
    const fileName = exportFileName(title, Date.now());
    downloadText(
      fileName,
      chatMarkdown(title, rows, exportLabels(t)),
    );
    toast.success(t('commands.exported', { file: fileName }));
    return true;
  };

  const chatCommands = useChatCommands({
    t,
    input,
    chatId,
    teamId: chatTeam.teamId,
    teams: chatTeam.teams,
    setTeamId: chatTeam.setTeamId,
    providers,
    agentProfiles,
    promptOptions,
    setMode: (value) => {
      setMode(value);
      setPromptId(AUTO);
    },
    setRangePreset,
    setExecutionPolicy: execution.choose,
    bypassAllowed: execution.bypassAllowed,
    setProviderId,
    setAgentProfileId,
    setPromptId,
    newChat,
    clearInput: () => setInput(''),
    showCommands: () => {
      setInput('/');
      window.requestAnimationFrame(() => composerRef.current?.focus());
    },
    exportChat,
    compactChat: compactCurrentChat,
  });

  // Permission errors remain distinct from request failures.
  const chatsErr = chatsQ.error ? toApiError(chatsQ.error) : null;
  if (chatsErr) {
    if (chatsErr.status === 403) {
      return <ProductState variant="permission-denied" />;
    }
    return <ProductState variant="error" error={chatsQ.error} />;
  }
  if (chatsQ.isLoading) {
    return <ProductState variant="loading" />;
  }

  const chats = chatsQ.data ?? [];
  const selectedProvider = providerId !== AUTO ? providers.find((p) => p.id === providerId) : null;
  const selectedAgentProfile =
    agentProfileId !== AUTO
      ? agentProfiles.find((profile) => profile.id === agentProfileId)
      : agentProfiles.find((profile) => profile.is_default);
  const agentProfileOptions = [
    { value: AUTO, label: t('advanced.profile_auto') },
    ...agentProfiles.map((profile) => ({
      value: profile.id,
      label: profile.name,
    })),
  ];
  const modelOptions = [
    { value: AUTO, label: t('model_auto') },
    ...providers.map((p) => ({ value: p.id, label: modelOptionLabel(p) })),
  ];
  const profile = profileQ.data;
  const displayName =
    profile?.display_name ||
    authCtx?.display_name ||
    profile?.email?.split('@')[0] ||
    authCtx?.email?.split('@')[0] ||
    authCtx?.user_id ||
    '';
  const chatUser: ChatUserIdentity = {
    id: authCtx?.user_id ?? '',
    displayName,
    avatarUrl: profile?.avatar_url?.trim() ?? '',
  };

  const selectChat = (chat: chatApi.Chat) => {
    shouldStickToBottomRef.current = true;
    setChatId(chat.id);
    setMode(chatModeForAnalysisMode(chat.analysis_mode));
    setPromptId(AUTO);
    setCapability(chat.capability ?? null);
    setRegeneratingFromMessageId(null);
    setMobileHistoryOpen(false);
  };

  // A fork opens at once, with the chat up to the answer it came from; the list shows it
  // under its source.
  const forkChat = (messageId: string) => {
    if (!chatId) return;
    chatApi
      .forkChat(chatId, messageId)
      .then(async (fork) => {
        await chatsQ.refetch();
        selectChat(fork);
        toast.success(t('forked'));
      })
      .catch((error: unknown) => toast.error(toApiError(error).message));
  };

  const deleteChat = async () => {
    const chat = chatPendingDeletion;
    if (!chat || deletingChat) return;
    setDeletingChat(true);
    try {
      await chatApi.deleteChat(chat.id);
      if (chat.id === chatId) newChat();
      await chatsQ.refetch();
      setChatPendingDeletion(null);
      toast.success(t('chat_deleted'));
    } catch (error) {
      toast.error(toApiError(error).message);
    } finally {
      setDeletingChat(false);
    }
  };

  // A question given back to the composer, with what it carried, to be changed and sent again.
  const editQuestion = (question: chatApi.ChatMessageRow) => {
    setInput(question.content);
    setReferences(question.references ?? []);
    window.requestAnimationFrame(() => composerRef.current?.focus());
  };

  const primeInput = (selection: StarterSelection) => {
    setInput(selection.prompt);
    if (selection.context) {
      setContext((current) => ({ ...current, ...selection.context }));
    }
    if (selection.rangePreset) setRangePreset(selection.rangePreset);
    if (selection.mode) {
      setMode(selection.mode);
      setPromptId(AUTO);
    }
    setCapability(selection.capability ?? null);
    if (selection.executionPolicy) {
      execution.choose(selection.executionPolicy);
    }
    window.requestAnimationFrame(() => composerRef.current?.focus());
  };

  const send = async ({
    content: overrideContent,
    regenerateFromMessageId,
  }: {
    content?: string;
    regenerateFromMessageId?: string;
  } = {}) => {
    const content = (overrideContent ?? input).trim();
    if (!content) return;
    const isRegeneration = Boolean(regenerateFromMessageId);
    // A control command runs here, also while an answer is coming; a task command is sent.
    if (!isRegeneration && chatCommands.runLocally(content) !== 'send') return;
    // A new question waits its turn behind the answer being written; a new version does not.
    if (isRegeneration && streaming) return;
    chatCommands.setNotice(null);
    shouldStickToBottomRef.current = true;
    setStreamError(null);
    setBudgetNotice(null);
    let chatIdValue = chatId;
    try {
      if (!chatIdValue) {
        const provider = providers.find((p) => p.id === providerId);
        const created = await chatApi.createChat({
          provider: provider?.provider ?? 'openai',
          model: provider?.default_model ?? '',
          title: titleForNewChat(titleSource(content), chats),
          title_provisional: true,
          provider_id: providerId !== AUTO ? providerId : undefined,
          analysis_mode: analysisModeForChatMode(mode),
          capability: capability ?? undefined,
          team_id: chatTeam.teamId ?? undefined,
          subject,
        });
        chatIdValue = created.id;
        setChatId(chatIdValue);
        setSubject(undefined);
        void chatsQ.refetch();
      }
    } catch (e) {
      setStreamError(toApiError(e).message);
      return;
    }

    const attached = isRegeneration ? [] : references;
    const body: chatApi.PostMessageBody = {
      content,
      regenerate_from_message_id: regenerateFromMessageId,
      time_range: timeRangeMicros(globalTimeWindow),
      analysis_mode: analysisModeForChatMode(mode),
      capability: capability ?? undefined,
      execution_policy: executionPolicy,
      stream_hints: contextStreamHints(context, globalFilters),
      agent_profile_id: agentProfileId !== AUTO ? agentProfileId : undefined,
      provider_id: providerId !== AUTO ? providerId : undefined,
      model: selectedProvider?.default_model,
      prompt_template_id: promptId !== AUTO ? promptId : undefined,
      references: attached.length > 0 ? attached : undefined,
    };
    if (!isRegeneration) {
      setInput('');
      setReferences([]);
      setPendingUser({
        chatId: chatIdValue,
        content,
        sentAtMicros: Date.now() * 1000,
      });
    } else {
      setPendingUser(null);
      setRegeneratingFromMessageId(regenerateFromMessageId ?? null);
    }

    const posted = await chatApi.postMessage(chatIdValue, body);
    // What was not kept goes back to the composer to be changed and sent again.
    const giveBack = () => {
      if (isRegeneration) return;
      setInput(content);
      setReferences(attached);
    };
    switch (posted.kind) {
      case 'queued':
        chatRun.follow(posted.run);
        break;
      case 'budget':
        setBudgetNotice(posted.notice);
        if (posted.notice.blocked) giveBack();
        break;
      case 'refused':
        chatCommands.refused(posted.refusal);
        giveBack();
        break;
      case 'error':
        setStreamError(posted.message);
        break;
    }
    await messagesQ.refetch();
    setPendingUser(null);
    setRegeneratingFromMessageId(null);
    void chatsQ.refetch();
  };

  // Stop the answer being written: the server stops calling the model and the tools.
  const stop = () => {
    chatRun.stop().catch((error: unknown) => toast.error(toApiError(error).message));
  };

  const messages = visibleChatMessages(messagesQ.data ?? []);
  const renderedConversation = conversationItems(messages);
  const pendingUserAlreadyRendered = pendingUser ? hasPersistedPendingUser(messages, pendingUser) : false;
  const showPostError = Boolean(streamError && chatId);
  const showStreamErrorBanner = Boolean(streamError && !chatId);
  const currentChat = chats.find((chat) => chat.id === chatId);
  const incidentChat = currentChat?.subject_kind === 'incident';
  const showEmptyWorkspace = !chatId && !pendingUser;

  return (
    <div
      data-agent-chat-workspace
      className={cn(
        'flex min-h-0 flex-col',
        // Embedded in the shell Mole Agent panel: fill the panel and stay
        // single-column. As a full route: fill the viewport below the chrome
        // and split into chat-list + chat at md+.
        embedded
          ? 'h-full'
          : 'h-full bg-[var(--page-canvas)] md:flex-row md:gap-[8px]',
      )}
    >
      {!embedded && (
        <ConversationHistory
          chats={chats}
          selectedChatId={chatId}
          streaming={streaming}
          width={chatListWidth}
          resizing={isChatListResizing}
          onNewChat={newChat}
          onSelectChat={selectChat}
          onDeleteChat={setChatPendingDeletion}
          teamName={chatTeam.teamName}
          onResizePointerDown={beginChatListResize}
          onResizeKeyDown={handleChatListResizeKeyDown}
          onResetWidth={resetChatListWidth}
        />
      )}

      {/* Main column */}
      <div
        id="agent-chat-transcript"
        data-testid="conversation-workspace"
        className={cn(
          'flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden bg-[var(--functional-surface)]',
          !embedded && 'rounded-md',
        )}
      >
        {!embedded && (
          <div className="flex h-11 shrink-0 items-center justify-between bg-[var(--control-surface)] px-2 md:justify-end xl:hidden">
            <button
              type="button"
              onClick={() => setMobileHistoryOpen(true)}
              className="inline-flex min-h-11 items-center gap-2 rounded-md px-2 type-caption font-strong text-tx-1 hover:bg-bg-2 focus-visible:bg-bg-2 md:hidden"
            >
              <MessageSquareText aria-hidden="true" className="h-4 w-4 text-tx-3" />
              {t('chats')}
            </button>
            <button
              type="button"
              onClick={() => setMobileContextOpen(true)}
              className="inline-flex min-h-11 items-center gap-2 rounded-md px-2 type-caption font-strong text-tx-1 hover:bg-bg-2 focus-visible:bg-bg-2"
            >
              <SlidersHorizontal aria-hidden="true" className="h-4 w-4 text-tx-3" />
              {t('workspace.context_title')}
            </button>
          </div>
        )}
        {messagePins.pinnedIds.length > 0 && (
          <div className="shrink-0 px-3 pt-1 md:px-5">
            <div className="mx-auto w-full max-w-[960px]">
              <PinnedAnswers
                pinnedIds={messagePins.pinnedIds}
                messages={messages}
                t={t}
                onJump={(messageId) =>
                  scrollRef.current
                    ?.querySelector(`[data-message-id="${CSS.escape(messageId)}"]`)
                    ?.scrollIntoView({ behavior: 'smooth', block: 'start' })
                }
              />
            </div>
          </div>
        )}
        <div
          ref={scrollRef}
          onScroll={handleTranscriptScroll}
          onWheelCapture={handleTranscriptWheel}
          onTouchStart={handleTranscriptTouchStart}
          onTouchMove={handleTranscriptTouchMove}
          className={cn(
            'min-h-0 flex-1 overflow-auto px-3 scroll-fade-y md:px-5',
            showEmptyWorkspace ? 'flex flex-col' : 'py-5',
          )}
        >
          {showEmptyWorkspace ? (
            <div className="mx-auto flex min-h-full w-full max-w-[960px] flex-col gap-3 py-3 md:py-4">
              <AgentWorkspace
                displayName={displayName}
                onPrime={primeInput}
              />
              {showStreamErrorBanner && (
                <div className="rounded-md border border-red/40 bg-red/5 p-3 font-sans text-sm text-red-soft">
                  {t('answer.failed_description')}
                </div>
              )}
            </div>
          ) : (
            <div className="mx-auto flex w-full max-w-[960px] flex-col gap-5">
              {renderedConversation.map((item) =>
                item.kind === 'turn' ? (
                  <ConversationTurnView
                    key={item.user.id}
                    turn={item}
                    t={t}
                    user={chatUser}
                    chatId={chatId}
                    feedback={feedbackByAnswer}
                    canRegenerate={!streaming}
                    regenerating={
                      regeneratingFromMessageId === item.user.id
                    }
                    queued={chatRun.queuedMessageIds.has(item.user.id)}
                    unanswered={chatRun.unansweredMessageIds.get(item.user.id) ?? null}
                    incidentChat={incidentChat}
                    onRegenerate={() => {
                      void send({
                        content: item.user.content,
                        regenerateFromMessageId: item.user.id,
                      });
                    }}
                    onEdit={() => editQuestion(item.user)}
                    pinnedIds={messagePins.pinnedIds}
                    onTogglePin={messagePins.toggle}
                    onFork={forkChat}
                  />
                ) : (
                  <MessageBubble
                    key={item.message.id}
                    role="assistant"
                    content={item.message.content}
                    t={t}
                    user={chatUser}
                    createdAtMicros={item.message.created_at_micros}
                    evidence={item.message.evidence_json}
                  />
                ),
              )}
              {pendingUser && !pendingUserAlreadyRendered && (
                <MessageBubble
                  role="user"
                  content={pendingUser.content}
                  pending
                  t={t}
                  user={chatUser}
                />
              )}
              {/* Once the answer is kept it shows in the chat; its live copy goes at once. */}
              {!messages.some(
                (message) =>
                  message.role === 'assistant' &&
                  Boolean(chatRun.view?.runId) &&
                  message.run_id === chatRun.view?.runId,
              ) && (
                <LiveRun
                  chatRun={chatRun}
                  user={chatUser}
                  viewerId={authCtx?.user_id ?? ''}
                  initiatorName={(id) => users.nameOf(id)}
                />
              )}
              {showPostError && streamError && (
                <MessageBubble
                  role="assistant"
                  content={formatStreamErrorContent(streamError)}
                  t={t}
                  user={chatUser}
                />
              )}
            </div>
          )}
        </div>

        <div className="shrink-0 px-3 pb-3 pt-2 md:px-5 md:pb-4">
          <div className="mx-auto w-full max-w-[960px]">
            {providersQ.isSuccess && providers.length === 0 && (
              // Nothing can answer yet. Said where the question is typed, before it is sent,
              // with the way to a model for whoever may set one up.
              <div
                data-agent-no-model
                role="status"
                className="mb-2 flex flex-wrap items-center gap-x-3 gap-y-1 rounded-md bg-[var(--yellow-tint)] px-3 py-2 font-sans text-xs text-[var(--yellow-ink)]"
              >
                <span className="font-strong">{t('workspace.no_model.title')}</span>
                <span className="min-w-0 flex-1">
                  {manageAccess.allowed
                    ? t('workspace.no_model.description')
                    : t('workspace.no_model.ask_admin')}
                </span>
                {manageAccess.allowed && (
                  <Link
                    to="/agent/settings/models"
                    className="shrink-0 rounded-md bg-bg-1 px-2.5 py-1 font-strong text-tx-0 hover:bg-bg-2 focus-visible:bg-bg-2"
                  >
                    {t('workspace.no_model.configure')}
                  </Link>
                )}
              </div>
            )}
            <ChatBudgetNotice notice={budgetNotice} />
            <ChatCommandNotice notice={chatCommands.notice} t={t} />
            <ChatComposer
              t={t}
              input={input}
              onInputChange={(value) => {
                setInput(value);
                chatCommands.setNotice(null);
              }}
              onSubmit={(content) => void send(content === undefined ? {} : { content })}
              commands={chatCommands.commands}
              argumentOptions={chatCommands.argumentOptions}
              streaming={streaming}
              onStop={stop}
              // On the page the list starts a new chat; the side panel has no list.
              {...(embedded && !showEmptyWorkspace ? { onNewChat: newChat } : {})}
              context={context}
              onContextChange={setContext}
              rangePreset={rangePreset}
              rangeLabel={rangeLabel}
              onRangeChange={setRangePreset}
              mode={mode}
              onModeChange={(value) => {
                setMode(value);
                setPromptId(AUTO);
              }}
              executionPolicy={executionPolicy}
              onExecutionPolicyChange={execution.choose}
              bypassAllowed={execution.bypassAllowed}
              agentProfileId={agentProfileId}
              onAgentProfileChange={setAgentProfileId}
              agentProfileOptions={agentProfileOptions}
              selectedAgentProfile={selectedAgentProfile}
              providerId={providerId}
              onProviderChange={setProviderId}
              modelOptions={modelOptions}
              promptId={promptId}
              onPromptChange={setPromptId}
              promptOptions={promptOptions}
              composerRef={composerRef}
              references={references}
              onReferencesChange={setReferences}
              {...((chatTeam.locked ? chatTeam.team : chatTeam.teams.length > 0)
                ? {
                    team: {
                      value: chatTeam.teamId ?? PERSONAL,
                      onChange: (value: string) =>
                        chatTeam.setTeamId(value === PERSONAL ? null : value),
                      options: [
                        { value: PERSONAL, label: t('team.personal') },
                        ...chatTeam.teams.map((team) => ({ value: team.id, label: team.name })),
                      ],
                      locked: chatTeam.locked,
                      label: chatTeam.team?.name ?? t('team.personal'),
                    },
                  }
                : {})}
            />
          </div>
        </div>
      </div>

      {!embedded && <SystemContext context={context} timeLabel={rangeLabel} />}

      {!embedded && (
        <>
          <Sheet open={mobileHistoryOpen} onOpenChange={setMobileHistoryOpen}>
            <SheetContent
              side="left"
              className="w-[min(88vw,320px)] border-bd-0 bg-bg-1 p-0 pt-10 shadow-none [&>button]:left-3 [&>button]:right-auto"
            >
              <SheetTitle className="sr-only">{t('chats')}</SheetTitle>
              <ConversationHistory
                variant="drawer"
                chats={chats}
                selectedChatId={chatId}
                streaming={streaming}
                onNewChat={newChat}
                onSelectChat={selectChat}
                onDeleteChat={setChatPendingDeletion}
                teamName={chatTeam.teamName}
              />
            </SheetContent>
          </Sheet>
          <Sheet open={mobileContextOpen} onOpenChange={setMobileContextOpen}>
            <SheetContent
              side="right"
              className="w-[min(88vw,320px)] border-bd-0 bg-bg-1 p-0 pt-10 shadow-none"
            >
              <SheetTitle className="sr-only">
                {t('workspace.context_title')}
              </SheetTitle>
              <SystemContext
                variant="drawer"
                context={context}
                timeLabel={rangeLabel}
              />
            </SheetContent>
          </Sheet>
        </>
      )}

      {execution.dialog}
      <ConfirmDialog
        open={chatPendingDeletion !== null}
        onOpenChange={(open) => {
          if (!open && !deletingChat) setChatPendingDeletion(null);
        }}
        destructive
        busy={deletingChat}
        title={t('chat_delete_title')}
        description={
          chatPendingDeletion
            ? t('chat_delete_confirm', {
                title:
                  displayTitleForChat(chatPendingDeletion, chats) ||
                  t('untitled'),
              })
            : undefined
        }
        cancelLabel={tc('actions.cancel')}
        confirmLabel={t('chat_delete_action')}
        busyLabel={t('chat_deleting')}
        onConfirm={deleteChat}
      />
    </div>
  );
}

function modelOptionLabel(provider: providersApi.ModelProvider): string {
  return provider.default_model;
}

export default AgentChat;

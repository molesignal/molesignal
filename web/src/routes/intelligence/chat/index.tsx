import { useQuery } from '@tanstack/react-query';
import type { TFunction } from 'i18next';
import {
  AlertTriangle,
  ArrowUp,
  Bell,
  Bot,
  CheckCircle2,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  CircleAlert,
  CircleGauge,
  Clock3,
  GitBranch,
  LayoutDashboard,
  Layers3,
  MessageSquareText,
  MoreHorizontal,
  Plus,
  RotateCcw,
  Settings2,
  ShieldCheck,
  SlidersHorizontal,
  Square,
  Trash2,
  UserRoundCheck,
  X,
} from 'lucide-react';
import * as React from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';

import { ConfirmDialog } from '@/admin';
import * as intelligenceApi from '@/api/intelligence';
import * as chatApi from '@/api/intelligence/chat';
import * as providersApi from '@/api/intelligence/modelProviders';
import * as promptsApi from '@/api/intelligence/prompts';
import * as meApi from '@/api/me';
import { toApiError } from '@/lib/http';
import { formatMicrosActive } from '@/lib/time';
import { ProductState } from '@/product/states';
import { ChromeButton } from '@/shell/chrome';
import { cn } from '@/shell/lib/cn';
import { LogoMark } from '@/shell/LogoMark';
import { Avatar, AvatarFallback, AvatarImage } from '@/shell/ui/avatar';
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from '@/shell/ui/popover';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/shell/ui/select';
import { toast } from '@/shell/ui/sonner';
import { useAuthStore } from '@/stores/auth';

import {
  fallbackToolLabel,
  formatInvestigationDuration,
  isRedundantInvestigationSummary,
  type InvestigationEvidenceItem,
  parseInvestigationEvidence,
  sanitizeAssistantContent,
} from '../agent/presentation';
import { type EvidenceRef, evidenceHref, parseStructuredAnswer } from '../answer';
import { dashboardStarterSelection } from '../dashboard-authoring/starter';
import { greetingPeriodForHour } from '../greeting';
import { MarkdownMessage } from '../markdown';
import {
  type ConversationTurn,
  conversationItems,
  formatStreamErrorContent,
  hasPersistedPendingUser,
  hasPersistedStreamError,
  type PendingUserMessage,
  visibleChatMessages,
} from '../messageRenderState';
import { discoverStarterService } from '../starterService';
import {
  CHAT_LIST_DEFAULT_WIDTH,
  CHAT_LIST_MAX_WIDTH,
  CHAT_LIST_MIN_WIDTH,
  CHAT_LIST_WIDTH_STORAGE_KEY,
  chatListWidthFromKey,
  chatListWidthFromPointer,
  clampChatListWidth,
  parseStoredChatListWidth,
} from './listResize';
import { displayTitleForChat, groupChats, titleForNewChat } from './presentation';

const TIME_PRESETS: Record<string, number> = {
  '15m': 15 * 60,
  '30m': 30 * 60,
  '1h': 3600,
  '6h': 6 * 3600,
  '24h': 24 * 3600,
  '7d': 7 * 86400,
};

const AUTO = 'auto';
const CHAT_MODES = ['auto', 'quick', 'deep', 'query_only'] as const;
const EXECUTION_POLICIES = ['advice_only', 'read_only', 'policy'] as const;
const STICKY_SCROLL_THRESHOLD_PX = 12;

type ChatMode = (typeof CHAT_MODES)[number];
type ExecutionPolicy = (typeof EXECUTION_POLICIES)[number];

interface ChatContext {
  environment: string;
  service: string;
  alert: string;
}

interface StarterSelection {
  prompt: string;
  context?: Partial<ChatContext>;
  rangePreset?: string;
  mode?: ChatMode;
  capability?: chatApi.ChatCapability;
  executionPolicy?: ExecutionPolicy;
}

interface LiveTool {
  id: string;
  name: string;
  status: 'running' | 'done' | 'error';
  arguments?: string;
  result?: string;
}

interface AggregatedTool {
  name: string;
  count: number;
  status: LiveTool['status'];
  calls: LiveTool[];
}

interface ChatUserIdentity {
  displayName: string;
  avatarUrl: string;
}

type ToolPayloadKind = 'arguments' | 'result' | 'error';

function rangeMicros(preset: string): chatApi.TimeRange {
  const nowMs = Date.now();
  const secs = TIME_PRESETS[preset] ?? 3600;
  return { start_micros: (nowMs - secs * 1000) * 1000, end_micros: nowMs * 1000 };
}

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

function chatModeForAnalysisMode(mode: string | null | undefined): ChatMode {
  if (mode === 'anomaly_analysis' || mode === 'alert_explain') return 'quick';
  if (mode === 'root_cause') return 'deep';
  if (mode === 'query_generation') return 'query_only';
  return 'auto';
}

function contextStreamHints(context: ChatContext): string[] {
  return [
    context.environment && `environment:${context.environment}`,
    context.service && `service:${context.service}`,
    context.alert && `alert:${context.alert}`,
  ].filter((value): value is string => Boolean(value));
}

function resizeComposer(textarea: HTMLTextAreaElement | null): void {
  if (!textarea) return;
  const maxHeight = 240;
  textarea.style.height = '0px';
  const nextHeight = Math.min(textarea.scrollHeight, maxHeight);
  textarea.style.height = `${nextHeight}px`;
  textarea.style.overflowY =
    textarea.scrollHeight > maxHeight ? 'auto' : 'hidden';
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

export function IntelligenceChat({ embedded = false }: { embedded?: boolean } = {}) {
  const { t } = useTranslation('intelligence');
  const { t: tc } = useTranslation('common');
  const authCtx = useAuthStore((s) => s.ctx);
  const chatsQ = useQuery({
    queryKey: ['intelligence', 'chats'],
    queryFn: () => chatApi.listChats(),
    retry: false,
  });
  const [chatId, setChatId] = React.useState<string | null>(null);
  const messagesQ = useQuery({
    queryKey: ['chat-messages', chatId],
    queryFn: () => chatApi.listMessages(chatId as string),
    enabled: !!chatId,
    retry: false,
  });
  const providersQ = useQuery({
    queryKey: ['intelligence', 'model-providers'],
    queryFn: () => providersApi.list(),
    retry: false,
  });
  const promptsQ = useQuery({
    queryKey: ['intelligence', 'prompts'],
    queryFn: () => promptsApi.list(),
    retry: false,
  });
  const agentProfilesQ = useQuery({
    queryKey: ['intelligence', 'agent-profiles'],
    queryFn: () => intelligenceApi.listProfiles(),
    retry: false,
  });
  const investigationsQ = useQuery({
    queryKey: ['intelligence', 'investigations'],
    queryFn: () => intelligenceApi.listInvestigations(),
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
  const [executionPolicy, setExecutionPolicy] =
    React.useState<ExecutionPolicy>('advice_only');
  const [rangePreset, setRangePreset] = React.useState('1h');
  const [agentProfileId, setAgentProfileId] = React.useState(AUTO);
  const [providerId, setProviderId] = React.useState(AUTO);
  const [promptId, setPromptId] = React.useState(AUTO);
  const [context, setContext] = React.useState<ChatContext>({
    environment: '',
    service: '',
    alert: '',
  });
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
  const [chatPendingDeletion, setChatPendingDeletion] =
    React.useState<chatApi.Chat | null>(null);
  const [deletingChat, setDeletingChat] = React.useState(false);

  const [streaming, setStreaming] = React.useState(false);
  const [pendingUser, setPendingUser] = React.useState<PendingUserMessage | null>(null);
  const [liveText, setLiveText] = React.useState('');
  const [liveTools, setLiveTools] = React.useState<LiveTool[]>([]);
  const [streamError, setStreamError] = React.useState<string | null>(null);
  const [regeneratingFromMessageId, setRegeneratingFromMessageId] =
    React.useState<string | null>(null);
  const abortRef = React.useRef<AbortController | null>(null);
  const scrollRef = React.useRef<HTMLDivElement | null>(null);
  const shouldStickToBottomRef = React.useRef(true);
  const lastTouchYRef = React.useRef<number | null>(null);
  const composerRef = React.useRef<HTMLTextAreaElement>(null);
  const chatListWidthRef = React.useRef(chatListWidth);
  const resizeCleanupRef = React.useRef<(() => void) | null>(null);

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
  }, [messagesQ.data, liveText, pendingUser, liveTools]);

  React.useEffect(() => () => abortRef.current?.abort(), []);

  React.useLayoutEffect(() => {
    resizeComposer(composerRef.current);
  }, [input]);

  // License / permission gate derived from the chats query error.
  const chatsErr = chatsQ.error ? toApiError(chatsQ.error) : null;
  if (chatsErr) {
    if (chatsErr.status === 403 && /licens/i.test(chatsErr.message)) {
      return (
        <ProductState
          variant="license-gated"
          title={t('unlicensed_title')}
          description={t('unlicensed_description')}
        />
      );
    }
    if (chatsErr.status === 403) {
      return <ProductState variant="permission-denied" />;
    }
    return <ProductState variant="error" error={chatsQ.error} />;
  }
  if (chatsQ.isLoading) {
    return <ProductState variant="loading" />;
  }

  const chats = chatsQ.data ?? [];
  const providers = (providersQ.data ?? []).filter((p) => p.enabled);
  const prompts = promptsQ.data ?? [];
  const agentProfiles = (agentProfilesQ.data ?? []).filter((profile) => profile.enabled);
  const investigations = investigationsQ.data ?? [];
  const selectedPurpose = purposeForChatMode(mode);
  const promptOptions = prompts.filter(
    (prompt) =>
      prompt.enabled &&
      prompt.purpose !== 'system' &&
      (!selectedPurpose || prompt.purpose === selectedPurpose),
  );
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
    displayName,
    avatarUrl: profile?.avatar_url?.trim() ?? '',
  };

  const newChat = () => {
    shouldStickToBottomRef.current = true;
    setChatId(null);
    setPendingUser(null);
    setLiveText('');
    setLiveTools([]);
    setStreamError(null);
    setRegeneratingFromMessageId(null);
    setCapability(null);
  };

  const selectChat = (chat: chatApi.Chat) => {
    shouldStickToBottomRef.current = true;
    setChatId(chat.id);
    setMode(chatModeForAnalysisMode(chat.analysis_mode));
    setPromptId(AUTO);
    setCapability(chat.capability ?? null);
    setRegeneratingFromMessageId(null);
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
      setExecutionPolicy(selection.executionPolicy);
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
    if (!content || streaming) return;
    const isRegeneration = Boolean(regenerateFromMessageId);
    shouldStickToBottomRef.current = true;
    setStreamError(null);
    let chatIdValue = chatId;
    try {
      if (!chatIdValue) {
        const provider = providers.find((p) => p.id === providerId);
        const created = await chatApi.createChat({
          provider: provider?.provider ?? 'openai',
          model: provider?.default_model ?? '',
          title: titleForNewChat(content, chats),
          provider_id: providerId !== AUTO ? providerId : undefined,
          analysis_mode: analysisModeForChatMode(mode),
          capability: capability ?? undefined,
        });
        chatIdValue = created.id;
        setChatId(chatIdValue);
        void chatsQ.refetch();
      }
    } catch (e) {
      setStreamError(toApiError(e).message);
      return;
    }

    const body: chatApi.PostMessageBody = {
      content,
      regenerate_from_message_id: regenerateFromMessageId,
      time_range: rangeMicros(rangePreset),
      analysis_mode: analysisModeForChatMode(mode),
      capability: capability ?? undefined,
      execution_policy: executionPolicy,
      stream_hints: contextStreamHints(context),
      agent_profile_id: agentProfileId !== AUTO ? agentProfileId : undefined,
      provider_id: providerId !== AUTO ? providerId : undefined,
      model: selectedProvider?.default_model,
      prompt_template_id: promptId !== AUTO ? promptId : undefined,
    };
    if (!isRegeneration) {
      setInput('');
      setPendingUser({
        chatId: chatIdValue,
        content,
        sentAtMicros: Date.now() * 1000,
      });
    } else {
      setPendingUser(null);
      setRegeneratingFromMessageId(regenerateFromMessageId ?? null);
    }
    setLiveText('');
    setLiveTools([]);
    setStreaming(true);
    const ac = new AbortController();
    abortRef.current = ac;
    await chatApi.postMessageStream(
      chatIdValue,
      body,
      {
        onChunk: (tx) => setLiveText((p) => p + tx),
        onToolStart: (e) =>
          setLiveTools((p) => [
            ...p,
            { id: e.id, name: e.name, status: 'running', arguments: e.arguments },
          ]),
        onToolEnd: (e) =>
          setLiveTools((p) =>
            p.map((x) =>
              x.id === e.id
                ? { ...x, status: e.is_error ? 'error' : 'done', result: e.result }
                : x,
            ),
          ),
        onError: (m) => setStreamError(m),
      },
      ac.signal,
    );
    setStreaming(false);
    setPendingUser(null);
    setRegeneratingFromMessageId(null);
    setLiveText('');
    await messagesQ.refetch();
    void chatsQ.refetch();
  };

  // 中断进行中的流式回复：abort 底层 fetch，后端检测到 SSE 断开后停止工具循环。
  const stop = () => {
    abortRef.current?.abort();
    setStreaming(false);
  };

  const messages = visibleChatMessages(messagesQ.data ?? []);
  const renderedConversation = conversationItems(messages);
  const lastTurn = [...renderedConversation]
    .reverse()
    .find((item): item is ConversationTurn => item.kind === 'turn');
  const pendingUserAlreadyRendered = pendingUser ? hasPersistedPendingUser(messages, pendingUser) : false;
  const persistedStreamError = streamError ? hasPersistedStreamError(messages, streamError) : false;
  const liveAssistantContent =
    streamError && !liveText.trim() ? formatStreamErrorContent(streamError) : liveText;
  const showLiveAssistant = streaming || Boolean(streamError && chatId && !persistedStreamError);
  const showStreamErrorBanner = Boolean(streamError && !chatId);
  const showEmptyWorkspace = !chatId && !pendingUser;
  const chatGroups = groupChats(chats);

  return (
    <div
      className={cn(
        'flex min-h-0 flex-col',
        // Embedded in the shell Mole Agent panel: fill the panel and stay
        // single-column. As a full route: fill the viewport below the chrome
        // and split into chat-list + chat at md+.
        embedded ? 'h-full' : 'h-full md:flex-row',
      )}
    >
      {/* Chat history — hidden in the narrow embedded panel. */}
      <aside
        className={cn('relative hidden shrink-0 flex-col border-r border-bd-0 bg-bg-1', !embedded && 'md:flex')}
        style={{ width: chatListWidth }}
      >
        <div className="flex items-center justify-between border-b border-bd-0 px-3 py-2.5">
          <span className="font-sans text-xs font-strong text-tx-1">{t('chats')}</span>
          <button
            type="button"
            onClick={newChat}
            className="inline-flex min-h-8 items-center gap-1 rounded-md px-2 font-sans text-xs font-strong text-indigo hover:bg-indigo/10"
          >
            <Plus className="h-3.5 w-3.5" /> {t('new_chat')}
          </button>
        </div>
        <div className="min-h-0 flex-1 overflow-auto px-2 pb-3">
          {chatGroups.map((group) => (
            <section key={group.key} className="pt-3">
              <h3 className="px-2 pb-1.5 font-sans text-type-micro font-strong uppercase tracking-[0.08em] text-tx-3">
                {t(`chat_groups.${group.key}`)}
              </h3>
              <div className="space-y-1">
                {group.chats.map((chat) => {
                  const active = chat.id === chatId;
                  const investigating = active && streaming;
                  const investigation = investigations.find(
                    (item) => item.chat_id === chat.id,
                  );
                  const isInvestigation = Boolean(chat.analysis_mode);
                  const ChatIcon = isInvestigation
                    ? CircleGauge
                    : MessageSquareText;
                  const statusLabel = investigating
                    ? t('conversation_status.investigating')
                    : investigation
                      ? t(`status.${investigation.status}`)
                      : t(
                          isInvestigation
                            ? 'conversation_status.investigation'
                            : 'conversation_status.chat',
                        );
                  return (
                    <div
                      key={chat.id}
                      className={cn(
                        'group/chat relative flex items-start rounded-r-md border-l-2 transition-colors',
                        active
                          ? 'border-indigo bg-indigo/10 text-tx-0'
                          : 'border-transparent text-tx-1 hover:bg-bg-2',
                      )}
                    >
                      <button
                        type="button"
                        onClick={() => selectChat(chat)}
                        className="flex min-w-0 flex-1 items-start gap-2.5 px-2 py-2 text-left"
                      >
                        <ChatIcon
                          className={cn(
                            'mt-0.5 h-3.5 w-3.5 shrink-0 text-tx-3',
                            investigating && 'text-indigo',
                          )}
                        />
                        <span className="min-w-0 flex-1">
                          <span className="block truncate font-sans text-xs font-strong">
                            {displayTitleForChat(chat, chats) || t('untitled')}
                          </span>
                          <span className="mt-1 block min-w-0 font-sans text-type-micro text-tx-3">
                            <span className="flex min-w-0 items-center justify-between gap-2">
                              <span
                                className={cn(
                                  'shrink-0',
                                  investigating && 'text-indigo',
                                )}
                              >
                                {statusLabel}
                              </span>
                              <span className="shrink-0">
                                {messageTime(chat.updated_at_micros)}
                              </span>
                            </span>
                          </span>
                        </span>
                      </button>
                      <button
                        type="button"
                        aria-label={t('chat_delete')}
                        title={t('chat_delete')}
                        onClick={() => setChatPendingDeletion(chat)}
                        className="mr-1 mt-1.5 grid h-7 w-7 shrink-0 place-items-center rounded-md text-tx-3 opacity-0 hover:bg-bg-1 hover:text-red focus-visible:opacity-100 group-hover/chat:opacity-100"
                      >
                        <Trash2 className="h-3.5 w-3.5" />
                      </button>
                    </div>
                  );
                })}
              </div>
            </section>
          ))}
        </div>
        <div
          role="separator"
          aria-orientation="vertical"
          aria-label={t('chat_list_resize')}
          aria-controls="intelligence-chat-transcript"
          aria-valuemin={CHAT_LIST_MIN_WIDTH}
          aria-valuemax={CHAT_LIST_MAX_WIDTH}
          aria-valuenow={chatListWidth}
          aria-valuetext={`${chatListWidth}px`}
          tabIndex={0}
          title={t('chat_list_resize')}
          data-testid="intelligence-chat-list-resizer"
          data-resizing={isChatListResizing || undefined}
          onPointerDown={beginChatListResize}
          onKeyDown={handleChatListResizeKeyDown}
          onDoubleClick={resetChatListWidth}
          className="group absolute inset-y-0 -right-1 z-20 w-2 touch-none select-none cursor-col-resize focus-visible:outline-none"
        >
          <span
            aria-hidden
            className={cn(
              'absolute inset-y-0 left-1/2 w-px -translate-x-1/2 bg-bd-0 transition-colors duration-fast',
              'group-hover:bg-indigo group-focus-visible:bg-indigo',
              isChatListResizing && 'bg-indigo',
            )}
          />
        </div>
      </aside>

      {/* Main column */}
      <div id="intelligence-chat-transcript" className="flex min-h-0 min-w-0 flex-1 flex-col">
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
            <div className="mx-auto flex min-h-full w-full max-w-[880px] flex-col justify-center gap-6 py-6">
              <Starter
                t={t}
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
                    canRegenerate={
                      item.user.id === lastTurn?.user.id && !streaming
                    }
                    regenerating={
                      regeneratingFromMessageId === item.user.id
                    }
                    onRegenerate={() => {
                      void send({
                        content: item.user.content,
                        regenerateFromMessageId: item.user.id,
                      });
                    }}
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
                <MessageBubble role="user" content={pendingUser.content} t={t} user={chatUser} />
              )}
              {showLiveAssistant && (
                <MessageBubble
                  role="assistant"
                  content={liveAssistantContent}
                  t={t}
                  user={chatUser}
                  streaming={streaming && !streamError}
                  liveTools={liveTools}
                  {...(pendingUser
                    ? {
                        investigationStartedAtMicros:
                          pendingUser.sentAtMicros,
                      }
                    : {})}
                />
              )}
            </div>
          )}
        </div>

        <div className="shrink-0 px-3 pb-3 pt-2 md:px-5 md:pb-4">
          <div className="mx-auto w-full max-w-[960px]">
            <ChatComposer
              t={t}
              input={input}
              onInputChange={setInput}
              onSubmit={() => void send()}
              streaming={streaming}
              onStop={stop}
              {...(!showEmptyWorkspace ? { onNewChat: newChat } : {})}
              context={context}
              onContextChange={setContext}
              rangePreset={rangePreset}
              onRangeChange={setRangePreset}
              mode={mode}
              onModeChange={(value) => {
                setMode(value);
                setPromptId(AUTO);
              }}
              executionPolicy={executionPolicy}
              onExecutionPolicyChange={setExecutionPolicy}
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
            />
          </div>
        </div>
      </div>
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

function Starter({
  t,
  displayName,
  onPrime,
}: {
  t: TFunction<'intelligence'>;
  displayName: string;
  onPrime: (selection: StarterSelection) => void;
}) {
  const orgId = useAuthStore((state) => state.ctx?.org_id ?? '');
  const starterServiceQ = useQuery({
    queryKey: ['intelligence', 'starter-service', orgId],
    queryFn: () => discoverStarterService({ orgId }),
    enabled: Boolean(orgId),
    staleTime: 2 * 60_000,
    retry: false,
  });
  const [greetingPeriod, setGreetingPeriod] = React.useState(() =>
    greetingPeriodForHour(new Date().getHours()),
  );

  React.useEffect(() => {
    const refreshGreeting = () => {
      setGreetingPeriod(greetingPeriodForHour(new Date().getHours()));
    };
    const timer = window.setInterval(refreshGreeting, 60_000);
    return () => window.clearInterval(timer);
  }, []);

  const name = displayName.trim();
  const greeting = name
    ? t(`greeting.${greetingPeriod}_named`, { name })
    : t(`greeting.${greetingPeriod}`);
  const suggestedService = starterServiceQ.data ?? '';
  const serviceErrorQuestion = suggestedService
    ? t('quick.service_errors', { service: suggestedService })
    : t('quick.service_errors_fallback');
  const suggestions = [
    {
      icon: AlertTriangle,
      label: serviceErrorQuestion,
      description: t('quick_descriptions.service_errors'),
      selection: {
        prompt: serviceErrorQuestion,
        ...(suggestedService
          ? { context: { service: suggestedService } }
          : {}),
        rangePreset: '1h',
        mode: 'deep' as const,
      },
    },
    {
      icon: GitBranch,
      label: t('quick.trace_anomalies'),
      description: t('quick_descriptions.trace_anomalies'),
      selection: {
        prompt: t('quick.trace_anomalies'),
        rangePreset: '30m',
        mode: 'deep' as const,
      },
    },
    {
      icon: Bell,
      label: t('quick.unacknowledged_alerts'),
      description: t('quick_descriptions.unacknowledged_alerts'),
      selection: {
        prompt: t('quick.unacknowledged_alerts'),
        mode: 'quick' as const,
      },
    },
    {
      icon: UserRoundCheck,
      label: t('quick.current_on_call'),
      description: t('quick_descriptions.current_on_call'),
      selection: {
        prompt: t('quick.current_on_call'),
        context: { environment: 'production' },
        mode: 'quick' as const,
      },
    },
    {
      icon: LayoutDashboard,
      label: t('quick.build_dashboard'),
      description: t('quick_descriptions.build_dashboard'),
      selection: dashboardStarterSelection(t('quick.build_dashboard')),
    },
  ];
  return (
    <div className="mx-auto flex w-full max-w-[800px] shrink-0 flex-col items-center px-2 text-center">
      <div className="grid h-9 w-9 place-items-center rounded-md border border-bd-0 bg-bg-1">
        <LogoMark size={22} />
      </div>
      <h2 className="mt-3 font-sans">
        <span className="block text-sm font-strong tracking-[-0.01em] text-tx-2">
          {greeting}
        </span>
        <span className="mt-1.5 block text-xl font-display-strong tracking-[-0.025em] text-tx-0 md:text-2xl">
          {t('empty_question')}
        </span>
      </h2>
      <p className="mt-2 max-w-xl whitespace-pre-line font-sans text-sm leading-6 text-tx-3">
        {t('empty_description')}
      </p>

      <div className="mt-5 w-full text-left">
        <div className="mb-2 font-sans text-xs font-strong uppercase tracking-[0.08em] text-tx-3">
          {t('quick_title')}
        </div>
        <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
          {suggestions.map((item) => {
            const Icon = item.icon;
            return (
              <button
                key={item.label}
                type="button"
                onClick={() => onPrime(item.selection)}
                className="group flex min-h-14 items-center gap-2.5 rounded-md border border-bd-0 bg-bg-1 px-3 py-2.5 text-left font-sans transition-colors duration-fast ease-default hover:border-bd-2 hover:bg-bg-2 focus-visible:border-bd-2 focus-visible:bg-bg-2"
              >
                <span className="grid h-7 w-7 shrink-0 place-items-center rounded-md border border-bd-0 bg-bg-2 text-tx-3 group-hover:text-indigo">
                  <Icon className="h-3.5 w-3.5" />
                </span>
                <span className="min-w-0">
                  <span className="block text-xs font-strong text-tx-1 group-hover:text-tx-0">
                    {item.label}
                  </span>
                  <span className="mt-0.5 hidden text-type-micro leading-4 text-tx-3 xl:block">
                    {item.description}
                  </span>
                </span>
              </button>
            );
          })}
        </div>
      </div>
    </div>
  );
}

function ChatComposer({
  t,
  input,
  onInputChange,
  onSubmit,
  streaming,
  onStop,
  onNewChat,
  context,
  onContextChange,
  rangePreset,
  onRangeChange,
  mode,
  onModeChange,
  executionPolicy,
  onExecutionPolicyChange,
  agentProfileId,
  onAgentProfileChange,
  agentProfileOptions,
  selectedAgentProfile,
  providerId,
  onProviderChange,
  modelOptions,
  promptId,
  onPromptChange,
  promptOptions,
  composerRef,
}: {
  t: TFunction<'intelligence'>;
  input: string;
  onInputChange: (value: string) => void;
  onSubmit: () => void;
  streaming: boolean;
  onStop: () => void;
  onNewChat?: () => void;
  context: ChatContext;
  onContextChange: React.Dispatch<React.SetStateAction<ChatContext>>;
  rangePreset: string;
  onRangeChange: (value: string) => void;
  mode: ChatMode;
  onModeChange: (value: ChatMode) => void;
  executionPolicy: ExecutionPolicy;
  onExecutionPolicyChange: (value: ExecutionPolicy) => void;
  agentProfileId: string;
  onAgentProfileChange: (value: string) => void;
  agentProfileOptions: Array<{ value: string; label: string }>;
  selectedAgentProfile: intelligenceApi.AgentProfile | undefined;
  providerId: string;
  onProviderChange: (value: string) => void;
  modelOptions: Array<{ value: string; label: string }>;
  promptId: string;
  onPromptChange: (value: string) => void;
  promptOptions: promptsApi.AgentPrompt[];
  composerRef: React.RefObject<HTMLTextAreaElement>;
}) {
  const hasExplicitContext = Boolean(
    context.environment || context.service || context.alert,
  );
  const clearField = (field: keyof ChatContext) => {
    onContextChange((current) => ({ ...current, [field]: '' }));
  };
  return (
    <form
      data-testid="composer-shell"
      className="overflow-hidden rounded-2xl border border-bd-1 bg-bg-1 shadow-sm"
      onSubmit={(event) => {
        event.preventDefault();
        onSubmit();
      }}
    >
      {hasExplicitContext && (
        <div
          className="flex flex-wrap items-center gap-1.5 px-3 pt-3"
          data-testid="composer-context"
        >
          {context.environment && (
            <ContextChip
              label={`${t('context.environment')}: ${context.environment}`}
              onRemove={() => clearField('environment')}
            />
          )}
          {context.service && (
            <ContextChip
              label={`${t('context.service')}: ${context.service}`}
              onRemove={() => clearField('service')}
            />
          )}
          {context.alert && (
            <ContextChip
              label={`${t('context.alert')}: ${context.alert}`}
              onRemove={() => clearField('alert')}
            />
          )}
        </div>
      )}
      <textarea
        value={input}
        onChange={(event) => {
          onInputChange(event.target.value);
          resizeComposer(event.currentTarget);
        }}
        onKeyDown={(event) => {
          if (event.key === 'Enter' && !event.shiftKey) {
            event.preventDefault();
            onSubmit();
          }
        }}
        rows={1}
        aria-label={t('composer_placeholder') as string}
        placeholder={t('composer_placeholder') as string}
        ref={composerRef}
        className="block max-h-[240px] min-h-11 w-full resize-none overflow-y-hidden border-0 bg-transparent px-3.5 py-3 font-sans text-base leading-6 text-tx-0 placeholder:text-tx-3 focus:outline-none md:text-sm"
      />
      <div
        data-testid="composer-controls"
        className="flex min-h-10 items-center gap-1 overflow-x-auto px-2 pb-2 whitespace-nowrap [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
      >
        <ContextEditorPopover
          t={t}
          context={context}
          onChange={onContextChange}
          triggerClassName="h-10 md:h-8"
        />
        <ControlSelect
          value={rangePreset}
          onChange={onRangeChange}
          ariaLabel={t('controls.time')}
          options={Object.keys(TIME_PRESETS).map((key) => ({
            value: key,
            label: t(`range.${key}`),
          }))}
        />
        <ControlSelect
          value={mode}
          onChange={(value) => onModeChange(value as ChatMode)}
          ariaLabel={t('controls.mode')}
          options={CHAT_MODES.map((item) => ({
            value: item,
            label: t(`mode.${item}`),
          }))}
        />
        <ControlSelect
          value={executionPolicy}
          onChange={(value) =>
            onExecutionPolicyChange(value as ExecutionPolicy)
          }
          ariaLabel={t('controls.execution')}
          options={EXECUTION_POLICIES.map((policy) => ({
            value: policy,
            label: t(`execution_policy.${policy}`),
          }))}
        />
        <AdvancedSettingsPopover
          t={t}
          agentProfileId={agentProfileId}
          onAgentProfileChange={onAgentProfileChange}
          agentProfileOptions={agentProfileOptions}
          selectedAgentProfile={selectedAgentProfile}
          providerId={providerId}
          onProviderChange={onProviderChange}
          modelOptions={modelOptions}
          promptId={promptId}
          onPromptChange={onPromptChange}
          promptOptions={promptOptions}
        />
        <span className="min-w-2 flex-1" />
        {onNewChat && (
          <button
            type="button"
            onClick={onNewChat}
            aria-label={t('new_chat')}
            title={t('new_chat')}
            className="grid h-10 w-10 shrink-0 place-items-center rounded-md text-tx-3 hover:bg-bg-3 hover:text-tx-0 md:h-8 md:w-8"
          >
            <Plus className="h-3.5 w-3.5" />
          </button>
        )}
        {streaming ? (
          <ChromeButton
            type="button"
            onClick={onStop}
            aria-label={t('stop')}
            title={t('stop')}
            className="h-10 w-10 shrink-0 justify-center rounded-full !p-0 md:h-9 md:w-9"
          >
            <Square className="h-3.5 w-3.5 fill-current" />
          </ChromeButton>
        ) : (
          <ChromeButton
            type="submit"
            variant="primary"
            disabled={!input.trim()}
            aria-label={t('send')}
            title={t('send')}
            className="h-10 w-10 shrink-0 justify-center rounded-full !p-0 disabled:cursor-not-allowed disabled:opacity-45 md:h-9 md:w-9"
          >
            <ArrowUp className="h-4 w-4" />
          </ChromeButton>
        )}
      </div>
    </form>
  );
}

function AdvancedSettingsPopover({
  t,
  agentProfileId,
  onAgentProfileChange,
  agentProfileOptions,
  selectedAgentProfile,
  providerId,
  onProviderChange,
  modelOptions,
  promptId,
  onPromptChange,
  promptOptions,
}: {
  t: TFunction<'intelligence'>;
  agentProfileId: string;
  onAgentProfileChange: (value: string) => void;
  agentProfileOptions: Array<{ value: string; label: string }>;
  selectedAgentProfile: intelligenceApi.AgentProfile | undefined;
  providerId: string;
  onProviderChange: (value: string) => void;
  modelOptions: Array<{ value: string; label: string }>;
  promptId: string;
  onPromptChange: (value: string) => void;
  promptOptions: promptsApi.AgentPrompt[];
}) {
  return (
    <Popover>
      <PopoverTrigger asChild>
        <button
          type="button"
          className="inline-flex h-10 shrink-0 items-center gap-1.5 rounded-md px-2.5 font-sans text-xs font-strong text-tx-2 hover:bg-bg-3 hover:text-tx-0 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo/30 md:h-8"
        >
          <SlidersHorizontal className="h-3.5 w-3.5" />
          {t('advanced.trigger')}
        </button>
      </PopoverTrigger>
      <PopoverContent
        align="end"
        side="top"
        className="w-[360px] max-w-[calc(100vw-24px)] p-0"
      >
        <div className="border-b border-bd-0 px-4 py-3">
          <div className="flex items-center gap-2">
            <Settings2 className="h-4 w-4 text-indigo" />
            <h3 className="text-sm font-strong text-tx-0">{t('advanced.title')}</h3>
          </div>
          <p className="mt-1 text-xs leading-5 text-tx-3">
            {t('advanced.description')}
          </p>
        </div>
        <div className="space-y-3 p-4">
          <AdvancedSelect
            label={t('advanced.profile')}
            value={agentProfileId}
            onChange={onAgentProfileChange}
            options={agentProfileOptions}
          />
          <AdvancedSelect
            label={t('model_prefix')}
            value={providerId}
            onChange={onProviderChange}
            options={modelOptions}
          />
          <AdvancedSelect
            label={t('prompt_prefix')}
            value={promptId}
            onChange={onPromptChange}
            options={[
              { value: AUTO, label: t('prompt_auto') },
              ...promptOptions.map((prompt) => ({
                value: prompt.id,
                label: prompt.name,
              })),
            ]}
          />
          {selectedAgentProfile && (
            <div className="grid grid-cols-2 gap-2 rounded-md border border-bd-0 bg-bg-2 p-3">
              <LimitSummary
                icon={Clock3}
                label={t('advanced.investigation_limit')}
                value={t('advanced.minutes', {
                  count: Math.max(
                    1,
                    Math.round(selectedAgentProfile.max_investigation_secs / 60),
                  ),
                })}
              />
              <LimitSummary
                icon={ShieldCheck}
                label={t('advanced.tool_limit')}
                value={t('advanced.calls', {
                  count: selectedAgentProfile.max_tool_calls,
                })}
              />
            </div>
          )}
        </div>
        <div className="border-t border-bd-0 px-4 py-2.5">
          <Link
            to="/intelligence/settings"
            className="inline-flex items-center gap-1.5 text-xs font-strong text-indigo hover:underline"
          >
            {t('advanced.manage_profile')}
          </Link>
        </div>
      </PopoverContent>
    </Popover>
  );
}

function AdvancedSelect({
  label,
  value,
  onChange,
  options,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  options: Array<{ value: string; label: string }>;
}) {
  return (
    <label className="block">
      <span className="mb-1.5 block text-xs font-strong text-tx-2">{label}</span>
      <Select value={value} onValueChange={onChange}>
        <SelectTrigger
          aria-label={label}
          className="h-9 w-full border-bd-1 bg-bg-1 text-sm shadow-none"
        >
          <SelectValue />
        </SelectTrigger>
        <SelectContent side="top" align="start">
          {options.map((option) => (
            <SelectItem key={option.value} value={option.value}>
              {option.label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </label>
  );
}

function LimitSummary({
  icon: Icon,
  label,
  value,
}: {
  icon: React.ElementType;
  label: string;
  value: string;
}) {
  return (
    <div className="min-w-0">
      <div className="flex items-center gap-1.5 text-type-micro text-tx-3">
        <Icon className="h-3.5 w-3.5" />
        <span className="truncate">{label}</span>
      </div>
      <div className="mt-1 text-xs font-strong text-tx-1">{value}</div>
    </div>
  );
}

function ContextChip({
  label,
  onRemove,
}: {
  label: string;
  onRemove?: () => void;
}) {
  return (
    <span className="inline-flex h-7 max-w-[240px] items-center gap-1 rounded-md border border-bd-0 bg-bg-2 px-2 font-sans text-type-micro text-tx-2">
      <span className="truncate">{label}</span>
      {onRemove && (
        <button
          type="button"
          aria-label={label}
          onClick={onRemove}
          className="-mr-1 grid h-5 w-5 shrink-0 place-items-center rounded text-tx-3 hover:bg-bg-3 hover:text-tx-0"
        >
          <X className="h-3 w-3" />
        </button>
      )}
    </span>
  );
}

function ContextEditorPopover({
  t,
  context,
  onChange,
  triggerClassName,
  side = 'top',
}: {
  t: TFunction<'intelligence'>;
  context: ChatContext;
  onChange: React.Dispatch<React.SetStateAction<ChatContext>>;
  triggerClassName?: string;
  side?: 'top' | 'bottom';
}) {
  const setField = (field: keyof ChatContext, value: string) => {
    onChange((current) => ({ ...current, [field]: value }));
  };
  return (
    <Popover>
      <PopoverTrigger asChild>
        <button
          type="button"
          className={cn(
            'inline-flex shrink-0 items-center gap-1.5 rounded-md px-2 font-sans text-xs font-strong text-tx-2 hover:bg-bg-2 hover:text-tx-0 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo/30',
            triggerClassName,
          )}
        >
          <Plus className="h-3.5 w-3.5" />
          {t('context.add')}
        </button>
      </PopoverTrigger>
      <PopoverContent align="start" side={side} className="w-[340px] max-w-[calc(100vw-24px)] p-4">
        <div className="flex items-start gap-3">
          <Layers3 className="mt-0.5 h-4 w-4 shrink-0 text-indigo" />
          <div>
            <h3 className="text-sm font-strong text-tx-0">
              {t('context.editor_title')}
            </h3>
            <p className="mt-1 text-xs leading-5 text-tx-3">
              {t('context.editor_description')}
            </p>
          </div>
        </div>
        <div className="mt-4 space-y-3">
          {(['environment', 'service', 'alert'] as const).map((field) => (
            <label key={field} className="block">
              <span className="mb-1.5 block text-xs font-strong text-tx-2">
                {t(`context.${field}`)}
              </span>
              <input
                value={context[field]}
                onChange={(event) => setField(field, event.target.value)}
                placeholder={t(`context.${field}_placeholder`)}
                className="h-9 w-full rounded-md border border-bd-1 bg-bg-2 px-3 font-sans text-sm text-tx-0 placeholder:text-tx-3 focus:outline-none"
              />
            </label>
          ))}
        </div>
        <button
          type="button"
          onClick={() =>
            onChange({ environment: '', service: '', alert: '' })
          }
          className="mt-3 inline-flex h-8 items-center gap-1.5 rounded-md px-2 text-xs text-tx-3 hover:bg-bg-2 hover:text-tx-0"
        >
          <RotateCcw className="h-3.5 w-3.5" />
          {t('context.clear')}
        </button>
      </PopoverContent>
    </Popover>
  );
}

function ThinkingIndicator({ t }: { t: (k: string) => string }) {
  return (
    <div className="flex items-center gap-3 text-tx-2">
      <div className="flex items-center gap-1">
        {[0, 1, 2].map((i) => (
          <span
            key={i}
            className="h-1.5 w-1.5 animate-pulse rounded-full bg-indigo"
            style={{ animationDelay: `${i * 120}ms` }}
          />
        ))}
      </div>
      <span className="text-shimmer">{t('thinking')}</span>
    </div>
  );
}

function modelOptionLabel(provider: providersApi.ModelProvider): string {
  return provider.default_model;
}

function InvestigationProcess({
  t,
  evidence = [],
  liveTools = [],
  running = false,
  startedAtMicros,
  finishedAtMicros,
}: {
  t: (k: string) => string;
  evidence?: InvestigationEvidenceItem[];
  liveTools?: LiveTool[];
  running?: boolean;
  startedAtMicros?: number;
  finishedAtMicros?: number;
}) {
  const [nowMs, setNowMs] = React.useState(() => Date.now());
  React.useEffect(() => {
    if (!running || startedAtMicros === undefined) return undefined;
    setNowMs(Date.now());
    const timer = window.setInterval(() => setNowMs(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [running, startedAtMicros]);
  const liveEvidence: InvestigationEvidenceItem[] = liveTools.map((tool) => ({
    toolCallId: tool.id,
    tool: tool.name,
    status:
      tool.status === 'done'
        ? 'success'
        : tool.status === 'error'
          ? 'error'
          : 'running',
    summary:
      summarizeToolPayload(
        tool.name,
        tool.result,
        tool.status === 'error' ? 'error' : 'result',
      )[0] ?? '',
    ...(tool.arguments ? { arguments: parseJson(tool.arguments) ?? tool.arguments } : {}),
  }));
  const items = liveEvidence.length > 0 ? liveEvidence : evidence;
  if (items.length === 0) return null;
  const toolDurationMs = items.reduce(
    (total, item) => total + (item.tookMs ?? 0),
    0,
  );
  const hasToolDuration = items.some((item) => item.tookMs !== undefined);
  const elapsedMs =
    startedAtMicros !== undefined
      ? Math.max(
          0,
          ((finishedAtMicros ?? nowMs * 1000) - startedAtMicros) / 1000,
        )
      : hasToolDuration
        ? toolDurationMs
        : undefined;
  const duration =
    elapsedMs === undefined ? '' : formatInvestigationDuration(elapsedMs);
  return (
    <details data-testid="investigation-process" className="group mt-3">
      <summary className="inline-flex min-h-11 cursor-pointer list-none items-center gap-2 rounded-md px-1.5 font-sans text-xs font-strong text-tx-2 transition-colors duration-fast hover:bg-bg-2 hover:text-tx-0 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo/30 md:min-h-9 [&::-webkit-details-marker]:hidden">
        <span aria-live="polite">
          {running
            ? t('investigation_process.processing')
            : t('investigation_process.processed')}
        </span>
        {duration && (
          <span className="font-mono font-normal tabular-nums text-tx-3">
            {duration}
          </span>
        )}
        <ChevronDown
          aria-hidden="true"
          className="h-3.5 w-3.5 shrink-0 text-tx-3 transition-transform duration-fast group-open:rotate-180"
        />
      </summary>
      <div className="mt-1 border-t border-bd-0 pt-1.5">
        <div className="space-y-1">
          {items.map((item) => (
            <InvestigationProcessRow key={item.toolCallId} item={item} t={t} />
          ))}
        </div>
      </div>
    </details>
  );
}

function InvestigationProcessRow({
  item,
  t,
}: {
  item: InvestigationEvidenceItem;
  t: (key: string) => string;
}) {
  const Icon =
    item.status === 'success'
      ? CheckCircle2
      : item.status === 'error'
        ? CircleAlert
        : CircleGauge;
  const translated = t(`tool_names.${item.tool}`);
  const displayName =
    translated === `tool_names.${item.tool}`
      ? fallbackToolLabel(item.tool)
      : translated;
  return (
    <div className="rounded-md px-2 py-2 hover:bg-bg-2">
      <div className="flex min-w-0 items-start gap-2">
        <Icon
          className={cn(
            'mt-0.5 h-3.5 w-3.5 shrink-0',
            item.status === 'success'
              ? 'text-green'
              : item.status === 'error'
                ? 'text-yellow-soft'
                : 'text-indigo',
          )}
        />
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-baseline gap-x-2 gap-y-0.5">
            <span className="text-xs font-strong text-tx-1">{displayName}</span>
            {item.rowCount !== undefined && (
              <span className="text-type-micro text-tx-3">
                {t('investigation_process.rows').replace(
                  '{{count}}',
                  String(item.rowCount),
                )}
              </span>
            )}
            {item.tookMs !== undefined && (
              <span className="text-type-micro text-tx-3">{item.tookMs}ms</span>
            )}
          </div>
          {item.summary &&
            item.summary !== 'ok' &&
            !isRedundantInvestigationSummary(item.summary, item.rowCount) && (
              <p className="mt-0.5 line-clamp-2 text-xs leading-5 text-tx-3">
                {item.summary}
              </p>
            )}
          <details className="mt-1">
            <summary className="cursor-pointer list-none text-type-micro text-tx-3 hover:text-tx-1 [&::-webkit-details-marker]:hidden">
              {t('investigation_process.technical_detail')}
            </summary>
            <div className="mt-1.5 rounded border border-bd-0 bg-bg-3 p-2 font-mono text-type-micro leading-5 text-tx-2">
              <div>{item.tool}</div>
              {item.arguments !== undefined && (
                <pre className="mt-1 max-h-28 overflow-auto whitespace-pre-wrap break-words">
                  {JSON.stringify(item.arguments, null, 2)}
                </pre>
              )}
            </div>
          </details>
        </div>
      </div>
    </div>
  );
}

export function aggregateTools(tools: LiveTool[]): AggregatedTool[] {
  const map = new Map<string, AggregatedTool>();
  for (const tool of tools) {
    const existing = map.get(tool.name);
    if (!existing) {
      map.set(tool.name, { name: tool.name, count: 1, status: tool.status, calls: [tool] });
      continue;
    }
    existing.count += 1;
    existing.status = mergeToolStatus(existing.status, tool.status);
    existing.calls.push(tool);
  }
  return [...map.values()];
}

function mergeToolStatus(a: LiveTool['status'], b: LiveTool['status']): LiveTool['status'] {
  if (a === 'running' || b === 'running') return 'running';
  if (a === 'error' || b === 'error') return 'error';
  return 'done';
}

export function summarizeToolPayload(
  toolName: string,
  value: string | undefined,
  kind: ToolPayloadKind,
): string[] {
  const trimmed = value?.trim() ?? '';
  if (!trimmed) return [];
  const parsed = parseJson(trimmed);
  if (parsed === null) return [capToolLine(trimmed)];

  const payloads = unwrapToolPayload(parsed);
  if (toolName === 'list_streams' && kind !== 'arguments') {
    return collectStreamNames(payloads).map(capToolLine);
  }
  if (kind === 'arguments') {
    return summarizeArguments(payloads);
  }
  return summarizeGenericPayload(payloads);
}

function parseJson(value: string): unknown | null {
  try {
    return JSON.parse(value) as unknown;
  } catch {
    return null;
  }
}

function unwrapToolPayload(value: unknown): unknown[] {
  if (Array.isArray(value)) {
    return value.flatMap((item) => {
      if (!isRecord(item)) return [item];
      if (item.type === 'json') return unwrapToolPayload(item.json);
      if (item.type === 'text') return typeof item.text === 'string' ? [item.text] : [];
      return [item];
    });
  }
  if (isRecord(value) && Array.isArray(value.content)) {
    return unwrapToolPayload(value.content);
  }
  return [value];
}

function summarizeArguments(payloads: unknown[]): string[] {
  return payloads.flatMap((payload) => {
    if (!isRecord(payload)) return summarizePrimitive(payload);
    return Object.entries(payload).flatMap(([key, value]) => summarizeField(key, value));
  });
}

function summarizeGenericPayload(payloads: unknown[]): string[] {
  const lines = payloads.flatMap((payload) => {
    if (!isRecord(payload)) return summarizePrimitive(payload);
    const countLines = ['rows', 'incidents', 'streams', 'spans', 'alerts']
      .flatMap((key) => {
        const value = payload[key];
        return Array.isArray(value) ? [`${key}: ${value.length}`] : [];
      });
    const fieldLines = Object.entries(payload)
      .filter(([key]) => !['rows', 'incidents', 'streams', 'spans', 'alerts'].includes(key))
      .flatMap(([key, value]) => summarizeField(key, value));
    return [...countLines, ...fieldLines];
  });
  return uniqueLines(lines.map(capToolLine));
}

function summarizeField(key: string, value: unknown): string[] {
  if (value === null || value === undefined || value === '') return [];
  if (key === 'time_range' && isRecord(value)) {
    const start = formatMicros(value.start_micros);
    const end = formatMicros(value.end_micros);
    if (start || end) return [`time_range: ${start || '?'} - ${end || '?'}`];
  }
  if (Array.isArray(value)) {
    if (value.length === 0) return [`${key}: 0`];
    const primitiveValues = value
      .map((item) => (typeof item === 'string' || typeof item === 'number' ? String(item) : null))
      .filter((item): item is string => Boolean(item));
    if (primitiveValues.length === value.length && primitiveValues.length <= 6) {
      return [`${key}: ${primitiveValues.join(', ')}`];
    }
    return [`${key}: ${value.length}`];
  }
  if (typeof value === 'string' || typeof value === 'number' || typeof value === 'boolean') {
    return [`${key}: ${String(value)}`];
  }
  return [];
}

function summarizePrimitive(value: unknown): string[] {
  if (typeof value === 'string') return value.trim() ? [value.trim()] : [];
  if (typeof value === 'number' || typeof value === 'boolean') return [String(value)];
  return [];
}

function collectStreamNames(payloads: unknown[]): string[] {
  const names = payloads.flatMap((payload) => collectStreamNamesFromValue(payload));
  return uniqueLines(names);
}

function collectStreamNamesFromValue(value: unknown): string[] {
  if (typeof value === 'string') return [value];
  if (Array.isArray(value)) return value.flatMap((item) => collectStreamNamesFromValue(item));
  if (!isRecord(value)) return [];
  const direct = typeof value.name === 'string'
    ? [value.name]
    : typeof value.stream === 'string'
      ? [value.stream]
      : typeof value.stream_name === 'string'
        ? [value.stream_name]
        : [];
  const nested = ['streams', 'items', 'data', 'results']
    .flatMap((key) => collectStreamNamesFromValue(value[key]));
  return [...direct, ...nested];
}

function formatMicros(value: unknown): string | null {
  if (typeof value !== 'number' || !Number.isFinite(value)) return null;
  // Tool payloads carry epoch micros (recent times exceed 1e13); older callers
  // may pass millis. Normalize to micros, then render via the central tz layer.
  const micros = value > 10_000_000_000_000 ? value : value * 1000;
  return formatMicrosActive(micros);
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function uniqueLines(lines: string[]): string[] {
  return [...new Set(lines.map((line) => line.trim()).filter(Boolean))];
}

function capToolLine(line: string): string {
  return line.length <= 180 ? line : `${line.slice(0, 180)}...`;
}

function ConversationTurnView({
  turn,
  t,
  user,
  canRegenerate,
  regenerating,
  onRegenerate,
}: {
  turn: ConversationTurn;
  t: (key: string) => string;
  user: ChatUserIdentity;
  canRegenerate: boolean;
  regenerating: boolean;
  onRegenerate: () => void;
}) {
  const [answerIndex, setAnswerIndex] = React.useState(
    Math.max(0, turn.answers.length - 1),
  );
  React.useEffect(() => {
    setAnswerIndex(Math.max(0, turn.answers.length - 1));
  }, [turn.answers.length]);
  const answer = turn.answers[answerIndex];
  const hasVersions = turn.answers.length > 1;
  return (
    <div className="flex flex-col gap-4" data-testid="conversation-turn">
      <MessageBubble
        role="user"
        content={turn.user.content}
        t={t}
        user={user}
        createdAtMicros={turn.user.created_at_micros}
      />
      {answer && (
        <MessageBubble
          role="assistant"
          content={answer.content}
          t={t}
          user={user}
          createdAtMicros={answer.created_at_micros}
          evidence={answer.evidence_json}
          investigationStartedAtMicros={turn.user.created_at_micros}
        />
      )}
      {(hasVersions || canRegenerate || regenerating) && (
        <div className="ml-10 flex min-h-7 flex-wrap items-center gap-1.5">
          {hasVersions && (
            <>
              <button
                type="button"
                aria-label={t('answer_versions.previous')}
                disabled={answerIndex === 0}
                onClick={() => setAnswerIndex((current) => Math.max(0, current - 1))}
                className="grid h-7 w-7 place-items-center rounded-md text-tx-3 hover:bg-bg-2 hover:text-tx-0 disabled:opacity-35"
              >
                <ChevronLeft className="h-3.5 w-3.5" />
              </button>
              <span className="inline-flex h-7 items-center gap-1 rounded-md px-1.5 text-xs text-tx-3">
                <MoreHorizontal className="h-3.5 w-3.5" />
                {t('answer_versions.position')
                  .replace('{{current}}', String(answerIndex + 1))
                  .replace('{{total}}', String(turn.answers.length))}
              </span>
              <button
                type="button"
                aria-label={t('answer_versions.next')}
                disabled={answerIndex >= turn.answers.length - 1}
                onClick={() =>
                  setAnswerIndex((current) =>
                    Math.min(turn.answers.length - 1, current + 1),
                  )
                }
                className="grid h-7 w-7 place-items-center rounded-md text-tx-3 hover:bg-bg-2 hover:text-tx-0 disabled:opacity-35"
              >
                <ChevronRight className="h-3.5 w-3.5" />
              </button>
            </>
          )}
          {(canRegenerate || regenerating) && (
            <button
              type="button"
              disabled={regenerating}
              onClick={onRegenerate}
              className="inline-flex h-7 items-center gap-1 rounded-md px-2 text-xs font-strong text-tx-3 hover:bg-bg-2 hover:text-tx-0 disabled:opacity-50"
            >
              <RotateCcw className={cn('h-3 w-3', regenerating && 'animate-spin')} />
              {regenerating
                ? t('answer_versions.regenerating')
                : t('answer_versions.regenerate')}
            </button>
          )}
        </div>
      )}
    </div>
  );
}

function MessageBubble({
  role,
  content,
  t,
  user,
  streaming,
  createdAtMicros,
  evidence,
  liveTools = [],
  investigationStartedAtMicros,
}: {
  role: string;
  content: string;
  t: (k: string) => string;
  user?: ChatUserIdentity;
  streaming?: boolean;
  createdAtMicros?: number;
  evidence?: unknown;
  liveTools?: LiveTool[];
  investigationStartedAtMicros?: number;
}) {
  const isUser = role === 'user';
  if (role === 'tool') return null;
  const displayContent = isUser ? content : sanitizeAssistantContent(content);
  const structured = !isUser ? parseStructuredAnswer(displayContent) : null;
  const persistedEvidence = parseInvestigationEvidence(evidence);
  const timestamp = createdAtMicros ? messageTime(createdAtMicros) : '';
  if (isUser) {
    return (
      <div className="flex flex-row-reverse gap-3" data-message-role="user">
        <div className="flex h-7 w-7 shrink-0 items-center justify-center overflow-hidden rounded-full bg-gradient-to-br from-indigo via-blue to-green text-white">
          <UserMessageAvatar user={user} />
        </div>
        <div className="flex max-w-[min(680px,84%)] min-w-0 flex-col items-end gap-1">
          <div className="min-w-0 rounded-lg border border-indigo/25 bg-indigo/10 px-3 py-2.5 font-sans text-base leading-6 text-tx-0 md:text-sm">
            <MarkdownMessage content={displayContent} />
          </div>
          {timestamp && <span className="text-type-micro text-tx-3">{timestamp}</span>}
        </div>
      </div>
    );
  }
  return (
    <div className="flex max-w-[960px] gap-3" data-message-role="assistant">
      <div className="flex h-7 w-7 shrink-0 items-center justify-center rounded-md border border-indigo/20 bg-indigo/10 text-indigo">
        <Bot className="h-4 w-4" />
      </div>
      <div className="min-w-0 flex-1">
        <div className="mb-2 flex items-baseline gap-2">
          <span className="text-sm font-display-strong tracking-[-0.01em] text-tx-0">
            Mole Agent
          </span>
          {timestamp && <span className="text-type-micro text-tx-3">{timestamp}</span>}
        </div>
        <div className="min-w-0 font-sans text-base leading-7 text-tx-1 md:text-sm md:leading-6">
          {streaming && !displayContent.trim() ? (
            <ThinkingIndicator t={t} />
          ) : isStreamErrorContent(displayContent) ? (
            <AgentFailure content={displayContent} t={t} />
          ) : structured ? (
            <StructuredView answer={structured} t={t} />
          ) : (
            <MarkdownMessage
              content={displayContent}
              streaming={Boolean(streaming)}
            />
          )}
        </div>
        <InvestigationProcess
          t={t}
          evidence={persistedEvidence}
          liveTools={liveTools}
          running={Boolean(streaming)}
          {...(investigationStartedAtMicros !== undefined
            ? { startedAtMicros: investigationStartedAtMicros }
            : {})}
          {...(!streaming && createdAtMicros !== undefined
            ? { finishedAtMicros: createdAtMicros }
            : {})}
        />
      </div>
    </div>
  );
}

function isStreamErrorContent(content: string): boolean {
  return /^\[error:[\s\S]*\]$/.test(content.trim());
}

function AgentFailure({
  content,
  t,
}: {
  content: string;
  t: (key: string) => string;
}) {
  const detail = content.trim().replace(/^\[error:\s*/, '').replace(/\]$/, '');
  return (
    <div>
      <p className="font-strong text-tx-0">{t('answer.failed_title')}</p>
      <p className="mt-1 text-tx-2">{t('answer.failed_description')}</p>
      <details className="mt-2 text-xs text-tx-3">
        <summary className="cursor-pointer">{t('answer.technical_error')}</summary>
        <pre className="mt-2 max-h-36 overflow-auto whitespace-pre-wrap rounded border border-bd-0 bg-bg-2 p-2 font-mono text-type-micro">
          {detail}
        </pre>
      </details>
    </div>
  );
}

function messageTime(micros: number): string {
  const value = new Date(micros / 1000);
  if (Number.isNaN(value.getTime())) return '';
  return value.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
}

function UserMessageAvatar({ user }: { user: ChatUserIdentity | undefined }) {
  const initial = (user?.displayName || 'M').trim()[0]?.toUpperCase() ?? 'M';
  return (
    <Avatar className="h-full w-full">
      {user?.avatarUrl && <AvatarImage src={user.avatarUrl} alt="" className="object-cover" />}
      <AvatarFallback className="bg-transparent font-sans text-xs font-bold text-white">
        {initial}
      </AvatarFallback>
    </Avatar>
  );
}

function StructuredView({
  answer,
  t,
}: {
  answer: ReturnType<typeof parseStructuredAnswer>;
  t: (k: string) => string;
}) {
  if (!answer) return null;
  return (
    <div className="flex flex-col gap-4">
      {answer.summary && (
        <Section title={t('answer.conclusion')}>
          <p className="font-strong text-tx-0">{answer.summary}</p>
        </Section>
      )}

      {answer.anomaly_points && answer.anomaly_points.length > 0 && (
        <Section title={t('answer.evidence')}>
          <ul className="flex flex-col gap-1">
            {answer.anomaly_points.map((a, i) => (
              <li key={i} className="rounded border border-bd-0 bg-bg-2 px-2 py-1">
                <span className="font-strong text-tx-0">{a.metric ?? a.stream ?? '—'}</span>
                {a.observed && (
                  <span className="text-tx-3">
                    {' '}
                    · {a.observed}
                    {a.expected ? ` (exp ${a.expected})` : ''}
                  </span>
                )}
                {a.description && <div className="text-tx-2">{a.description}</div>}
              </li>
            ))}
          </ul>
        </Section>
      )}

      {answer.evidence && answer.evidence.length > 0 && (
        <Section title={t('answer.evidence')}>
          <ul className="flex flex-col gap-1">
            {answer.evidence.map((ev, i) => (
              <EvidenceRow key={i} ev={ev} />
            ))}
          </ul>
        </Section>
      )}

      {answer.likely_causes && answer.likely_causes.length > 0 && (
        <Section title={t('answer.likely_causes')}>
          <ol className="ml-4 list-decimal space-y-0.5">
            {answer.likely_causes.map((c, i) => (
              <li key={i}>{c}</li>
            ))}
          </ol>
        </Section>
      )}

      {answer.limitations && answer.limitations.length > 0 && (
        <Section title={t('answer.limitations')}>
          <ul className="space-y-1">
            {answer.limitations.map((limitation, index) => (
              <li key={index} className="flex items-start gap-2 text-tx-2">
                <CircleAlert className="mt-1 h-3.5 w-3.5 shrink-0 text-yellow-soft" />
                <span>{limitation}</span>
              </li>
            ))}
          </ul>
        </Section>
      )}

      {answer.suggested_next_steps && answer.suggested_next_steps.length > 0 && (
        <Section title={t('answer.next_steps')}>
          <ol className="ml-4 list-decimal space-y-0.5">
            {answer.suggested_next_steps.map((c, i) => (
              <li key={i}>{c}</li>
            ))}
          </ol>
        </Section>
      )}

      {answer.related_links && answer.related_links.length > 0 && (
        <Section title={t('answer.related_links')}>
          <ul className="flex flex-wrap gap-2">
            {answer.related_links.map((l, i) => {
              const href = l.href ?? l.route;
              return (
                <li key={i}>
                  {href ? (
                    <Link
                      to={href}
                      className="inline-flex h-8 items-center rounded-md border border-bd-1 px-2.5 text-xs font-strong text-indigo hover:bg-bg-2"
                    >
                      {l.label}
                    </Link>
                  ) : (
                    <span className="inline-flex h-8 items-center rounded-md border border-bd-0 px-2.5 text-xs text-tx-2">
                      {l.label}
                    </span>
                  )}
                </li>
              );
            })}
          </ul>
        </Section>
      )}

      {answer.confidence !== undefined && (
        <div className="flex items-center gap-2 text-xs">
          <span className="text-tx-3">{t('answer.confidence')}</span>
          <span className="font-strong text-tx-0">
            {t(`answer.confidence_levels.${qualitativeConfidence(answer.confidence)}`)}
          </span>
        </div>
      )}
    </div>
  );
}

function qualitativeConfidence(value: number | 'high' | 'medium' | 'low'): 'high' | 'medium' | 'low' {
  if (typeof value === 'string') return value;
  if (value >= 0.75) return 'high';
  if (value >= 0.45) return 'medium';
  return 'low';
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div>
      <div className="mb-1 font-sans text-xs font-strong uppercase tracking-wide text-tx-3">
        {title}
      </div>
      {children}
    </div>
  );
}

function EvidenceRow({ ev }: { ev: EvidenceRef }) {
  const href = evidenceHref(ev);
  const label =
    ev.label ?? ev.stream ?? ev.trace_id ?? ev.object_key ?? ev.kind ?? 'evidence';
  return (
    <li className="rounded border border-bd-0 bg-bg-2 px-2 py-1">
      {href ? (
        <Link to={href} className="text-indigo hover:underline">
          {label}
        </Link>
      ) : (
        <span className="text-tx-2">{label}</span>
      )}
      {ev.query && <span className="ml-1 font-mono text-xs text-tx-3">{ev.query}</span>}
    </li>
  );
}

function ControlSelect({
  value,
  onChange,
  options,
  className,
  ariaLabel,
}: {
  value: string;
  onChange: (v: string) => void;
  options: Array<{ value: string; label: string }>;
  className?: string;
  ariaLabel?: string;
}) {
  const selectedLabel =
    options.find((option) => option.value === value)?.label ?? value;
  return (
    <Select
      value={value}
      onValueChange={onChange}
    >
      <SelectTrigger
        aria-label={ariaLabel}
        className={cn(
          'h-10 w-auto max-w-[200px] shrink-0 rounded-md !border-0 bg-transparent px-2.5 font-sans text-xs font-strong text-tx-1 shadow-none outline-none hover:bg-bg-3 hover:text-tx-0 focus:outline-none focus:ring-0 focus:ring-offset-0 data-[state=open]:bg-bg-3 md:h-8 [&>span]:min-w-0 [&>span]:truncate [&>svg]:ml-1.5 [&>svg]:h-3.5 [&>svg]:w-3.5 [&>svg]:text-indigo [&>svg]:opacity-100',
          className,
        )}
      >
        <SelectValue>{selectedLabel}</SelectValue>
      </SelectTrigger>
      <SelectContent side="top" align="start" sideOffset={6} className="max-w-[min(360px,calc(100vw-32px))]">
        {options.map((opt) => (
          <SelectItem key={opt.value} value={opt.value} className="font-sans text-xs">
            {opt.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}

export default IntelligenceChat;

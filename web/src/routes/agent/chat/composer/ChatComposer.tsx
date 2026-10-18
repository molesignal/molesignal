import type { TFunction } from 'i18next';
import { ArrowUp, Plus, Square, Users } from 'lucide-react';
import * as React from 'react';

import type * as agentApi from '@/api/agent';
import type { MessageReference } from '@/api/agent/chat';
import type { AgentCommand } from '@/api/agent/commands';
import type * as promptsApi from '@/api/agent/prompts';
import { AGENT_TIME_PRESETS } from '@/investigation/agentContext';
import { ChromeButton } from '@/shell/chrome';

import { AgentSettingsMenu } from './AdvancedSettings';
import { CommandMenu, optionId } from './CommandMenu';
import { type ArgumentOption, completeCommand } from './commands';
import { type ComposerHandle, type ComposerKeyHandler, ComposerEditor } from './ComposerEditor';
import { ContextChip, ContextEditorPopover } from './ContextControls';
import { ControlSelect } from './ControlSelect';
import { ExecutionModeMenu } from './ExecutionModeMenu';
import { commandMenu, menuKey, menuLength } from './menu';
import { ReferenceChip } from './references/ReferenceChips';
import { ReferenceMenu, useReferenceMention } from './references/useReferenceMention';
import {
  type ChatContext,
  type ChatMode,
  type ExecutionPolicy,
} from '../workspace/types';

/**
 * Where the question is typed, with the controls of the chat under it. A `/`
 * at the start opens the menu of commands, an `@` the things to attach; the
 * full page and the ⌘J panel share it.
 */
export function ChatComposer({
  t,
  input,
  onInputChange,
  onSubmit,
  commands,
  argumentOptions,
  streaming,
  onStop,
  onNewChat,
  context,
  onContextChange,
  rangePreset,
  rangeLabel,
  onRangeChange,
  mode,
  onModeChange,
  executionPolicy,
  onExecutionPolicyChange,
  bypassAllowed,
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
  team,
  references,
  onReferencesChange,
}: {
  t: TFunction<'agent'>;
  input: string;
  onInputChange: (value: string) => void;
  /** Sends what is typed, or the text given: a command chosen from the menu runs at once. */
  onSubmit: (content?: string) => void;
  /** The slash commands the menu offers. */
  commands: readonly AgentCommand[];
  /** The suggestions for a command's argument. */
  argumentOptions: (command: AgentCommand) => readonly ArgumentOption[];
  streaming: boolean;
  onStop: () => void;
  onNewChat?: () => void;
  context: ChatContext;
  onContextChange: React.Dispatch<React.SetStateAction<ChatContext>>;
  rangePreset: string;
  rangeLabel: string;
  onRangeChange: (value: string) => void;
  mode: ChatMode;
  onModeChange: (value: ChatMode) => void;
  executionPolicy: ExecutionPolicy;
  onExecutionPolicyChange: (value: ExecutionPolicy) => void;
  /** The chat's Agent Profile lets it pick Bypass permissions. */
  bypassAllowed: boolean;
  agentProfileId: string;
  onAgentProfileChange: (value: string) => void;
  agentProfileOptions: Array<{ value: string; label: string }>;
  selectedAgentProfile: agentApi.AgentProfile | undefined;
  providerId: string;
  onProviderChange: (value: string) => void;
  modelOptions: Array<{ value: string; label: string }>;
  promptId: string;
  onPromptChange: (value: string) => void;
  promptOptions: promptsApi.AgentPrompt[];
  composerRef: React.RefObject<ComposerHandle>;
  /**
   * Which team the chat is for: chosen before the first message, then shown.
   * Left out when the person has no team and the chat is personal.
   */
  team?: {
    value: string;
    options: Array<{ value: string; label: string }>;
    onChange: (value: string) => void;
    locked: boolean;
    label: string;
  };
  /** What is attached to the question with @. */
  references: readonly MessageReference[];
  onReferencesChange: (references: MessageReference[]) => void;
}) {
  const hasExplicitContext = Boolean(
    context.environment || context.service || context.alert,
  );
  const clearField = (field: keyof ChatContext) => {
    onContextChange((current) => ({ ...current, [field]: '' }));
  };

  const menuId = React.useId();
  // The text as the menus read it (plain, without its Markdown) and where the caret is in it.
  const [typed, setTyped] = React.useState({ text: input, caret: input.length });
  const { text, caret } = typed;
  // The text the menu was closed on with Escape: it stays closed until the text changes.
  const [dismissedAt, setDismissedAt] = React.useState<string | null>(null);
  const [active, setActive] = React.useState(0);
  const menu =
    dismissedAt === text ? null : commandMenu(text, caret, commands, argumentOptions);
  const rows = menuLength(menu);
  const key = menuKey(menu);
  React.useEffect(() => setActive(0), [key]);

  const mention = useReferenceMention({
    input: text,
    caret,
    references,
    onReferencesChange,
    onRemoveMention: (found) => composerRef.current?.deleteBeforeCaret(found.query.length + 1),
  });

  // Puts the text in the composer with the caret at its end, so the menu follows it. The caret
  // goes to the end of whatever the composer holds by then: running a command may change it.
  const fill = (next: string) => {
    onInputChange(next);
    window.requestAnimationFrame(() => composerRef.current?.focus());
  };

  // A command or an argument chosen from the menu goes into the composer, where more can be
  // written after it; Enter there sends it. A chosen argument closes the menu over it.
  const choose = (index: number) => {
    if (menu?.kind === 'commands') {
      const command = menu.commands[index];
      if (!command?.available) return;
      fill(completeCommand(command));
      return;
    }
    if (menu?.kind === 'arguments') {
      const option = menu.options[index];
      if (!option) return;
      const chosen = `/${menu.command.name} ${option.value}`;
      fill(chosen);
      setDismissedAt(chosen);
    }
  };

  // The keys of the menus first; then Enter sends, except in a list or in code, where it goes
  // on with them; ⌘/Ctrl+Enter sends from anywhere.
  const onKeyDown: ComposerKeyHandler = (event, { structured }) => {
    if (mention.onKeyDown(event)) return true;
    if (menu && event.key === 'Escape') {
      // Only the menu closes; the panel around the chat stays.
      event.preventDefault();
      event.stopPropagation();
      setDismissedAt(text);
      return true;
    }
    if (rows > 0) {
      if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
        event.preventDefault();
        const step = event.key === 'ArrowDown' ? 1 : -1;
        setActive((current) => (current + step + rows) % rows);
        return true;
      }
      if ((event.key === 'Enter' && !event.shiftKey) || event.key === 'Tab') {
        event.preventDefault();
        choose(active);
        return true;
      }
    }
    if (event.key !== 'Enter' || event.shiftKey) return false;
    if (structured && !event.metaKey && !event.ctrlKey) return false;
    event.preventDefault();
    onSubmit();
    return true;
  };

  return (
    <div className="relative">
      {mention.open ? (
        <div className="absolute inset-x-0 bottom-full z-30 mb-2">
          <ReferenceMenu
            query={mention.query}
            candidates={mention.candidates}
            active={mention.active}
            loading={mention.loading}
            error={mention.error}
            onActivate={mention.setActive}
            onChoose={mention.choose}
          />
        </div>
      ) : (
        menu && (
          <div className="absolute inset-x-0 bottom-full z-30 mb-2">
            <CommandMenu
              t={t}
              id={menuId}
              menu={menu}
              active={active}
              onActivate={setActive}
              onChoose={choose}
            />
          </div>
        )
      )}
      <form
        data-testid="composer-shell"
        aria-label={t('workspace.command_bar')}
        className="flex flex-col gap-1"
        onSubmit={(event) => {
          event.preventDefault();
          onSubmit();
        }}
      >
        {/* The question is written on a card of its own that floats over the chat, sent from
            inside it; what shapes the answer sits on a plain line under it. */}
        <div
          data-agent-composer-field
          className="overflow-hidden rounded-xl bg-[var(--floating-surface)] shadow-md"
        >
          {(hasExplicitContext || references.length > 0) && (
            <div
              className="flex flex-wrap items-center gap-1.5 px-3 pt-2.5"
              data-testid="composer-context"
            >
              {references.map((reference) => (
                <ReferenceChip
                  key={`${reference.kind}:${reference.id}`}
                  reference={reference}
                  onRemove={() =>
                    onReferencesChange(
                      references.filter(
                        (item) => !(item.kind === reference.kind && item.id === reference.id),
                      ),
                    )
                  }
                />
              ))}
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
          <div className="flex items-end gap-1.5 pr-2">
            <ComposerEditor
              ref={composerRef}
              value={input}
              onChange={(next) => {
                setTyped({ text: next.text, caret: next.caret });
                onInputChange(next.markdown);
              }}
              onCaret={(next) => setTyped({ text: next.text, caret: next.caret })}
              onKeyDown={onKeyDown}
              ariaLabel={t('composer_label')}
              placeholder={t('composer_placeholder')}
              expanded={rows > 0 || mention.open}
              controls={menu ? menuId : undefined}
              activeDescendant={rows > 0 ? optionId(menuId, active) : undefined}
            />
            <span className="flex shrink-0 items-center gap-1.5 pb-2">
              {streaming && (
                <ChromeButton
                  type="button"
                  onClick={onStop}
                  aria-label={t('stop')}
                  title={t('stop')}
                  className="h-10 w-10 shrink-0 justify-center rounded-full !p-0 md:h-8 md:w-8"
                >
                  <Square className="h-3.5 w-3.5 fill-current" />
                </ChromeButton>
              )}
              {/* While an answer is written a new question waits its turn behind it. */}
              {(!streaming || input.trim()) && (
                <ChromeButton
                  type="submit"
                  variant="primary"
                  disabled={!input.trim()}
                  aria-label={streaming ? t('queue_send') : t('send')}
                  title={streaming ? t('queue_send') : t('send')}
                  className="h-10 w-10 shrink-0 justify-center rounded-full !p-0 disabled:cursor-not-allowed disabled:opacity-45 md:h-8 md:w-8"
                >
                  <ArrowUp className="h-4 w-4" />
                </ChromeButton>
              )}
            </span>
          </div>
        </div>
        <div
          data-testid="composer-controls"
          className="flex min-h-9 items-center gap-0.5 overflow-x-auto bg-transparent px-1 whitespace-nowrap [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
        >
          <ContextEditorPopover
            t={t}
            context={context}
            onChange={onContextChange}
            triggerClassName="h-10 md:h-8"
          />
          {team &&
            (team.locked ? (
              <span
                data-testid="composer-team"
                className="inline-flex h-10 max-w-[200px] shrink-0 items-center gap-1.5 px-2.5 font-sans text-xs font-strong text-tx-1 md:h-8"
              >
                <Users aria-hidden="true" className="h-3.5 w-3.5 shrink-0 text-indigo" />
                <span className="truncate">{team.label}</span>
              </span>
            ) : (
              <ControlSelect
                value={team.value}
                onChange={team.onChange}
                ariaLabel={t('controls.team')}
                options={team.options}
              />
            ))}
          <ControlSelect
            value={rangePreset}
            onChange={onRangeChange}
            ariaLabel={t('controls.time')}
            options={[
              ...Object.keys(AGENT_TIME_PRESETS).map((key) => ({
                value: key,
                label: t(`range.${key}`),
              })),
              ...(rangePreset === 'custom'
                ? [{ value: 'custom', label: rangeLabel }]
                : []),
            ]}
          />
          <ExecutionModeMenu
            t={t}
            value={executionPolicy}
            onChange={onExecutionPolicyChange}
            bypassAllowed={bypassAllowed}
          />
          <span className="min-w-2 flex-1" />
          <AgentSettingsMenu
            t={t}
            mode={mode}
            onModeChange={onModeChange}
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
          {onNewChat && (
            <button
              type="button"
              onClick={onNewChat}
              aria-label={t('new_chat')}
              title={t('new_chat')}
              className="grid h-10 w-10 shrink-0 place-items-center rounded-md text-tx-3 hover:bg-bg-2 hover:text-tx-0 md:h-8 md:w-8"
            >
              <Plus className="h-3.5 w-3.5" />
            </button>
          )}
        </div>
      </form>
    </div>
  );
}

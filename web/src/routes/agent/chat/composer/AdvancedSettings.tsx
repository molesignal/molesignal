import type { TFunction } from 'i18next';
import { Check, MoreHorizontal } from 'lucide-react';
import { useNavigate } from 'react-router-dom';

import type * as agentApi from '@/api/agent';
import type * as promptsApi from '@/api/agent/prompts';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuItemIndicator,
  DropdownMenuLabel,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from '@/shell/ui/dropdown-menu';

import { AUTO } from './controlCommands';
import { CHAT_MODES, type ChatMode } from '../workspace/types';

interface Choice {
  value: string;
  label: string;
  description?: string;
}

/**
 * The rest of what shapes an answer, behind "More": one line per setting with
 * its current value, the choices one step further, and the way to the profiles.
 * A dot on the trigger says when anything differs from the defaults.
 */
export function AgentSettingsMenu({
  t,
  mode,
  onModeChange,
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
  t: TFunction<'agent'>;
  mode: ChatMode;
  onModeChange: (value: ChatMode) => void;
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
}) {
  const navigate = useNavigate();
  const hasOverrides =
    mode !== 'auto'
    || agentProfileId !== AUTO
    || providerId !== AUTO
    || promptId !== AUTO;
  const limits = selectedAgentProfile
    ? [
        `${t('advanced.investigation_limit')} ${t('advanced.minutes', {
          count: Math.max(1, Math.round(selectedAgentProfile.max_investigation_secs / 60)),
        })}`,
        `${t('advanced.tool_limit')} ${t('advanced.calls', {
          count: selectedAgentProfile.max_tool_calls,
        })}`,
      ].join(' · ')
    : null;

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <button
          type="button"
          className="inline-flex h-10 shrink-0 items-center gap-1.5 rounded-md px-2.5 font-sans text-xs font-strong text-tx-2 hover:bg-bg-3 hover:text-tx-0 focus-visible:bg-bg-3 focus-visible:text-tx-0 data-[state=open]:bg-bg-3 data-[state=open]:text-tx-0 md:h-8"
        >
          <MoreHorizontal aria-hidden="true" className="h-3.5 w-3.5" />
          {t('advanced.trigger')}
          {hasOverrides && (
            <span
              aria-label={t('advanced.overrides_active')}
              className="h-1.5 w-1.5 rounded-full bg-indigo"
            />
          )}
        </button>
      </DropdownMenuTrigger>
      <DropdownMenuContent
        side="top"
        align="end"
        sideOffset={6}
        className="w-[280px] max-w-[calc(100vw-32px)] !shadow-lg"
      >
        <DropdownMenuLabel>{t('advanced.title')}</DropdownMenuLabel>
        <Setting
          label={t('advanced.agent_mode')}
          value={mode}
          onChange={(value) => onModeChange(value as ChatMode)}
          choices={CHAT_MODES.map((item) => ({
            value: item,
            label: t(`mode.${item}`),
            description: t(`mode_descriptions.${item}`),
          }))}
        />
        <Setting
          label={t('advanced.profile')}
          value={agentProfileId}
          onChange={onAgentProfileChange}
          choices={agentProfileOptions}
          note={limits}
        />
        <Setting
          label={t('model_prefix')}
          value={providerId}
          onChange={onProviderChange}
          choices={modelOptions}
        />
        <Setting
          label={t('prompt_prefix')}
          value={promptId}
          onChange={onPromptChange}
          choices={[
            { value: AUTO, label: t('prompt_auto') },
            ...promptOptions.map((prompt) => ({ value: prompt.id, label: prompt.name })),
          ]}
        />
        <DropdownMenuSeparator />
        <DropdownMenuItem onSelect={() => navigate('/agent/settings')} className="text-indigo">
          {t('advanced.manage_profile')}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

/** One setting: its name and current value on a line, its choices in the menu beside it. */
function Setting({
  label,
  value,
  onChange,
  choices,
  note = null,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
  choices: Choice[];
  /** What the current choice brings with it, under the choices. */
  note?: string | null;
}) {
  const current = choices.find((choice) => choice.value === value)?.label ?? value;
  return (
    <DropdownMenuSub>
      <DropdownMenuSubTrigger
        aria-label={`${label}: ${current}`}
        className="gap-3 [&>svg:last-child]:ml-0 [&>svg:last-child]:text-tx-3"
      >
        <span className="shrink-0 text-tx-2">{label}</span>
        <span className="ml-auto min-w-0 truncate font-strong text-tx-0">{current}</span>
      </DropdownMenuSubTrigger>
      <DropdownMenuSubContent className="w-[280px] max-w-[calc(100vw-32px)] !shadow-lg">
        <DropdownMenuRadioGroup value={value} onValueChange={onChange}>
          {choices.map((choice) => (
            <DropdownMenuRadioItem
              key={choice.value}
              value={choice.value}
              indicator="none"
              className="items-start gap-3 py-[6px] data-[state=checked]:bg-transparent data-[state=checked]:font-body data-[state=checked]:text-tx-1"
            >
              <span className="min-w-0 flex-1">
                <span className="block truncate font-strong text-tx-0">{choice.label}</span>
                {choice.description && (
                  <span className="mt-0.5 block text-tx-3">{choice.description}</span>
                )}
              </span>
              <DropdownMenuItemIndicator className="pt-0.5">
                <Check aria-hidden="true" className="h-3.5 w-3.5 text-indigo-soft" />
              </DropdownMenuItemIndicator>
            </DropdownMenuRadioItem>
          ))}
        </DropdownMenuRadioGroup>
        {note && <p className="type-micro px-[10px] pb-1.5 pt-1 text-tx-3">{note}</p>}
      </DropdownMenuSubContent>
    </DropdownMenuSub>
  );
}

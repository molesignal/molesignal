import { useQuery } from '@tanstack/react-query';
import type { TFunction } from 'i18next';
import * as React from 'react';

import type { AgentProfile } from '@/api/agent/agentProfiles';
import {
  type AgentCommand,
  type CommandRefusal,
  listCommands,
} from '@/api/agent/commands';
import type { ModelProvider } from '@/api/agent/modelProviders';
import type { AgentPrompt } from '@/api/agent/prompts';
import * as incidentsApi from '@/api/incidents';
import type { Team } from '@/api/teams';
import { toast } from '@/shell/ui/sonner';

import { type ArgumentOption, findCommand, parseInput } from './commands';
import {
  AUTO,
  PERSONAL,
  modeFor,
  pickByName,
  policyFor,
  timePresetFor,
} from './controlCommands';
import type { ChatMode, ExecutionPolicy } from '../workspace/types';

/** Why a command was not run or sent, said over the composer until the text changes. */
export interface CommandNotice {
  code:
    | 'unknown_command'
    | 'client_command'
    | 'argument_missing'
    | 'argument_invalid'
    | 'name_not_found'
    | 'unavailable'
    | 'target_not_found'
    | 'nothing_to_export'
    | 'nothing_to_compact'
    | 'ambiguous'
    | 'needs_runner'
    | 'needs_credential'
    | 'failed';
  command: string;
  /** The words or the hint for the argument, the reason it is unavailable, or the name not found. */
  detail?: string;
}

export interface ChatCommandTargets {
  t: TFunction<'agent'>;
  /** What is typed: the incidents `/rca` offers are read only while it is typed. */
  input: string;
  /** The chat on screen; its skills and MCP prompts depend on whose it is. */
  chatId: string | null;
  /** The team the chat about to start is for. */
  teamId: string | null;
  /** The teams a chat can be started for: the person's own, every team for an administrator. */
  teams: readonly Team[];
  /** Sets the team of the chat about to start; null for a personal chat. */
  setTeamId: (id: string | null) => void;
  providers: readonly ModelProvider[];
  agentProfiles: readonly AgentProfile[];
  promptOptions: readonly AgentPrompt[];
  setMode: (mode: ChatMode) => void;
  setRangePreset: (preset: string) => void;
  setExecutionPolicy: (policy: ExecutionPolicy) => void;
  /** The chat's Agent Profile lets it pick Bypass permissions. */
  bypassAllowed: boolean;
  setProviderId: (id: string) => void;
  setAgentProfileId: (id: string) => void;
  setPromptId: (id: string) => void;
  newChat: () => void;
  /** Empties the composer once a control command has run. */
  clearInput: () => void;
  /** Opens the menu of every command. */
  showCommands: () => void;
  /** Downloads the chat; false when there is nothing in it yet. */
  exportChat: () => boolean;
  /** Compacts the chat so far into a summary; false when there is nothing in it yet. */
  compactChat: () => boolean;
}

const RCA_TYPED = /^\s*\/rca(\s|$)/i;

/** What became of the text: run here, stopped with a notice, or to be sent. */
export type CommandOutcome = 'ran' | 'stopped' | 'send';

/**
 * The slash commands of the chat: the list the menu offers, the suggestions
 * for their arguments, and the control commands, run here without a round
 * trip. Task commands go to the backend with the message.
 */
export function useChatCommands(targets: ChatCommandTargets) {
  const { t } = targets;
  const scope = targets.chatId
    ? { chatId: targets.chatId }
    : { teamId: targets.teamId };
  const commandsQ = useQuery({
    queryKey: ['agent', 'commands', targets.chatId ?? null, targets.chatId ? null : targets.teamId],
    queryFn: () => listCommands(scope),
    retry: false,
    staleTime: 5 * 60_000,
  });
  const incidentsQ = useQuery({
    queryKey: ['agent', 'command-incidents'],
    queryFn: () => incidentsApi.list({ scope: 'active' }),
    enabled: RCA_TYPED.test(targets.input),
    retry: false,
    staleTime: 30_000,
  });
  const commands = commandsQ.data ?? [];
  const [notice, setNotice] = React.useState<CommandNotice | null>(null);

  const argumentOptions = (command: AgentCommand): ArgumentOption[] => {
    switch (command.name) {
      case 'mode':
        return command.choices.map((value) => ({
          value,
          label: t(`mode.${modeFor(value) ?? 'auto'}`),
        }));
      case 'time':
        return command.choices.map((value) => ({ value, label: t(`range.${value}`) }));
      case 'policy':
        return command.choices.map((value) => ({
          value,
          label: t(`execution_policy.${policyFor(value) ?? 'advice_only'}`),
        }));
      default:
        break;
    }
    switch (command.suggest) {
      case 'models':
        return [
          { value: AUTO, label: t('model_auto') },
          ...targets.providers.map((provider) => ({
            value: provider.name,
            label: provider.default_model,
          })),
        ];
      case 'profiles':
        return [
          { value: AUTO, label: t('advanced.profile_auto') },
          ...targets.agentProfiles.map((profile) => ({ value: profile.name })),
        ];
      case 'prompts':
        return [
          { value: AUTO, label: t('prompt_auto') },
          ...targets.promptOptions.map((prompt) => ({ value: prompt.name })),
        ];
      case 'incidents':
        return (incidentsQ.data ?? []).map((incident) => ({
          value: incident.id,
          label: incident.summary,
        }));
      case 'teams':
        return [
          { value: PERSONAL, label: t('team.personal') },
          ...targets.teams.map((team) => ({ value: team.name })),
        ];
      default:
        return command.choices.map((value) => ({ value }));
    }
  };

  const stop = (next: CommandNotice): CommandOutcome => {
    setNotice(next);
    return 'stopped';
  };

  const invalid = (command: AgentCommand) =>
    stop({ code: 'argument_invalid', command: command.name, detail: command.choices.join(' | ') });

  // The composer empties first: `/help` puts a slash back in it.
  const run = (apply: () => void, label?: string): CommandOutcome => {
    targets.clearInput();
    apply();
    if (label) toast.success(t('commands.applied', { value: label }));
    return 'ran';
  };

  const pick = <T extends { id: string }>(
    command: AgentCommand,
    items: readonly T[],
    namesOf: (item: T) => readonly string[],
    args: string,
    apply: (id: string) => void,
    autoLabel: string,
  ): CommandOutcome => {
    const picked = pickByName(items, namesOf, args);
    if (!picked) return stop({ code: 'name_not_found', command: command.name, detail: args });
    return run(
      () => apply(picked === AUTO ? AUTO : picked.id),
      picked === AUTO ? autoLabel : (namesOf(picked)[0] ?? args),
    );
  };

  /** Runs the text here when it is a control command, or says what is wrong with a command. */
  const runLocally = (raw: string): CommandOutcome => {
    const parsed = parseInput(raw);
    if (parsed.kind === 'text') return 'send';
    // Without the list the backend judges the command.
    if (!commandsQ.isSuccess) return 'send';

    const command = findCommand(commands, parsed);
    // A team's skill the list does not name this way may still be the chat's: the backend judges.
    if (!command && parsed.team) return 'send';
    if (!command) return stop({ code: 'unknown_command', command: parsed.name });
    if (!command.available) {
      return stop({
        code: 'unavailable',
        command: command.name,
        detail: command.unavailable_reason ?? '',
      });
    }
    if (command.argument_required && !parsed.args) {
      return stop({
        code: 'argument_missing',
        command: command.name,
        detail: command.argument_hint ?? '',
      });
    }
    if (command.kind === 'task') return 'send';

    const { args } = parsed;
    switch (command.name) {
      case 'help':
        return run(targets.showCommands);
      case 'new':
        return run(targets.newChat);
      case 'mode': {
        const mode = modeFor(args);
        return mode ? run(() => targets.setMode(mode), t(`mode.${mode}`)) : invalid(command);
      }
      case 'time': {
        const preset = timePresetFor(args);
        return preset
          ? run(() => targets.setRangePreset(preset), t(`range.${preset}`))
          : invalid(command);
      }
      case 'policy': {
        const policy = policyFor(args);
        if (!policy) return invalid(command);
        // Bypass permissions is the Agent Profile's to allow, and is asked about before it is on.
        if (policy === 'bypass_permissions') {
          return targets.bypassAllowed
            ? run(() => targets.setExecutionPolicy(policy))
            : stop({
                code: 'unavailable',
                command: command.name,
                detail: t('execution_mode.bypass_locked'),
              });
        }
        return run(() => targets.setExecutionPolicy(policy), t(`execution_policy.${policy}`));
      }
      case 'model':
        return pick(
          command,
          targets.providers,
          (provider) => [provider.name, provider.default_model],
          args,
          targets.setProviderId,
          t('model_auto'),
        );
      case 'profile':
        return pick(
          command,
          targets.agentProfiles,
          (profile) => [profile.name],
          args,
          targets.setAgentProfileId,
          t('advanced.profile_auto'),
        );
      case 'prompt':
        return pick(
          command,
          targets.promptOptions,
          (prompt) => [prompt.name],
          args,
          targets.setPromptId,
          t('prompt_auto'),
        );
      case 'export':
        return targets.exportChat()
          ? run(() => undefined)
          : stop({ code: 'nothing_to_export', command: command.name });
      case 'compact':
        return targets.compactChat()
          ? run(() => undefined)
          : stop({ code: 'nothing_to_compact', command: command.name });
      case 'team': {
        // The team is chosen before a chat starts; in a chat that has started, a new one starts.
        const choose = (id: string | null, label: string) =>
          run(() => {
            if (targets.chatId) targets.newChat();
            targets.setTeamId(id);
          }, label);
        if (args.trim().toLowerCase() === PERSONAL) return choose(null, t('team.personal'));
        const picked = pickByName(targets.teams, (team) => [team.name], args);
        if (!picked || picked === AUTO) {
          return stop({ code: 'name_not_found', command: command.name, detail: args });
        }
        return choose(picked.id, picked.name);
      }
      default:
        // A control command this composer does not know how to run.
        return stop({ code: 'client_command', command: command.name });
    }
  };

  /** What the backend said about a command it refused. */
  const refused = (refusal: CommandRefusal) => {
    const codes: Record<CommandRefusal['code'], CommandNotice['code']> = {
      unknown_command: 'unknown_command',
      client_command: 'client_command',
      command_argument_missing: 'argument_missing',
      command_unavailable: 'unavailable',
      command_target_not_found: 'target_not_found',
      command_ambiguous: 'ambiguous',
      command_needs_runner: 'needs_runner',
      command_needs_credential: 'needs_credential',
      command_failed: 'failed',
    };
    const code = codes[refusal.code];
    const command = commands.find(
      (item) => item.name.toLowerCase() === refusal.command.toLowerCase(),
    );
    const detail = (() => {
      switch (code) {
        case 'argument_missing':
          return refusal.options.join(', ') || (command?.argument_hint ?? '');
        case 'unavailable':
          return command?.unavailable_reason ?? '';
        case 'ambiguous':
          return refusal.options.map((option) => `/${option}`).join(', ');
        case 'needs_credential':
          return command?.server ?? '';
        default:
          return '';
      }
    })();
    setNotice({ code, command: refusal.command, detail });
  };

  return { commands, argumentOptions, runLocally, refused, notice, setNotice };
}

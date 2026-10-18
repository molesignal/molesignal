import { http } from '@/lib/http';

/** Who carries a command out: the composer itself, or the backend in a round of the chat. */
export type CommandKind = 'control' | 'task';

/** Where a command comes from: built in, a skill, or a prompt of an MCP server. */
export type CommandSource = 'builtin' | 'skill' | 'mcp';

/** Where the suggestions for a command's argument come from: things the workspace has. */
export type CommandSuggest = 'models' | 'profiles' | 'prompts' | 'incidents' | 'teams';

/** A slash command of the chat, as the backend lists it for the signed-in person. */
export interface AgentCommand {
  name: string;
  kind: CommandKind;
  source: CommandSource;
  /** In the reader's language. */
  description: string;
  /** How the argument is written: the words it can be, joined by `|`, or a hint. */
  argument_hint: string | null;
  argument_required: boolean;
  /** The words the argument can be; empty when it is free text. */
  choices: string[];
  suggest: CommandSuggest | null;
  available: boolean;
  /** Why the command cannot be used, in the reader's language. */
  unavailable_reason: string | null;
  /** The team of a team skill, as written after `@`; the menu groups by it. */
  team?: string | null;
  /** The MCP server a prompt comes from; the menu groups by it. */
  server?: string | null;
}

/** The command a user message was, kept next to what was typed. */
export interface MessageCommand {
  name: string;
  args: string;
  source: CommandSource;
}

/**
 * The commands of a chat: the skills and MCP prompts depend on whose resources
 * the chat loads. An existing chat is named by its id; a chat not started yet
 * by the team it will be for, or neither for a personal one.
 */
export async function listCommands(
  scope: { chatId?: string | null; teamId?: string | null } = {},
): Promise<AgentCommand[]> {
  const params: Record<string, string> = {};
  if (scope.chatId) params.chat_id = scope.chatId;
  else if (scope.teamId) params.team_id = scope.teamId;
  const { data } = await http.get<{ commands: AgentCommand[] }>('/agent/commands', { params });
  return data.commands ?? [];
}

/** Why the backend refused a message that was a command. */
export type CommandRefusalCode =
  | 'unknown_command'
  | 'client_command'
  | 'command_argument_missing'
  | 'command_unavailable'
  | 'command_target_not_found'
  | 'command_ambiguous'
  | 'command_needs_runner'
  | 'command_needs_credential'
  | 'command_failed';

const REFUSAL_CODES: readonly CommandRefusalCode[] = [
  'unknown_command',
  'client_command',
  'command_argument_missing',
  'command_unavailable',
  'command_target_not_found',
  'command_ambiguous',
  'command_needs_runner',
  'command_needs_credential',
  'command_failed',
];

export interface CommandRefusal {
  code: CommandRefusalCode;
  command: string;
  /**
   * What to write instead when a skill name is ambiguous (`name@team`), or the
   * argument an MCP prompt still needs.
   */
  options: string[];
}

/** A refusal of a command read from the response body, or null when it is something else. */
export function commandRefusal(body: string): CommandRefusal | null {
  let payload: Record<string, unknown>;
  try {
    payload = JSON.parse(body) as Record<string, unknown>;
  } catch {
    return null;
  }
  const code = REFUSAL_CODES.find((item) => item === payload.code);
  if (!code) return null;
  const options = Array.isArray(payload.options)
    ? payload.options.filter((option): option is string => typeof option === 'string')
    : [];
  return { code, command: String(payload.command ?? ''), options };
}

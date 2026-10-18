/**
 * Slash commands in the chat composer: what the typed text is, and which
 * commands and arguments to offer while it is being typed.
 *
 * The backend reads a message by the same rules
 * (`crates/modules/agent/src/commands/parse.rs`):
 * - after leading spaces, `/` and a letter start a command; the word runs to
 *   the first space and holds only letters, digits, `_`, `-` and `:`; the name
 *   is not case sensitive and the rest, trimmed, is the argument;
 * - the name can be followed by `@team` (to the first space) to name a team's
 *   skill;
 * - a word with anything else in it (`/var/log/app.log`) is plain text;
 * - `\/` at the start sends the slash as text, without the backslash.
 */
import type { AgentCommand, CommandSource } from '@/api/agent/commands';

export type ParsedInput =
  | { kind: 'text'; text: string }
  | { kind: 'command'; name: string; team?: string; args: string };

const COMMAND_WORD = /^[A-Za-z][A-Za-z0-9_:-]*$/;
const NAME_CHARS = /^[A-Za-z0-9_:-]*(@[^\s/]*)?$/;

export function parseInput(raw: string): ParsedInput {
  const trimmed = raw.trimStart();
  if (trimmed.startsWith('\\/')) return { kind: 'text', text: trimmed.slice(1) };
  if (!trimmed.startsWith('/')) return { kind: 'text', text: raw };

  const body = trimmed.slice(1);
  const space = body.search(/\s/);
  const word = space === -1 ? body : body.slice(0, space);
  const at = word.indexOf('@');
  const name = at === -1 ? word : word.slice(0, at);
  const team = at === -1 ? undefined : word.slice(at + 1);
  if (team !== undefined && (!team || team.includes('/'))) return { kind: 'text', text: raw };
  if (!COMMAND_WORD.test(name)) return { kind: 'text', text: raw };

  return {
    kind: 'command',
    name: name.toLowerCase(),
    ...(team !== undefined ? { team } : {}),
    args: space === -1 ? '' : body.slice(space).trim(),
  };
}

/** The name a parsed command goes by in the list: `name`, or `name@team` for a team's skill. */
export function invocationOf(parsed: { name: string; team?: string }): string {
  return parsed.team ? `${parsed.name}@${parsed.team.toLowerCase()}` : parsed.name;
}

/** The command of the list a parsed command names, ignoring case. */
export function findCommand(
  commands: readonly AgentCommand[],
  parsed: { name: string; team?: string },
): AgentCommand | undefined {
  const wanted = invocationOf(parsed);
  return commands.find((command) => command.name.toLowerCase() === wanted);
}

/** What a user message says, the way it reads in the chat: an escaped slash without its backslash. */
export function displayText(raw: string): string {
  const parsed = parseInput(raw);
  return parsed.kind === 'text' ? parsed.text : raw;
}

/**
 * The part of a command name typed so far while the caret is still in it
 * (`/mo|` → "mo", `/` → ""), or null when the text is not a command being named.
 */
export function commandQuery(raw: string, caret: number): string | null {
  if (!raw.startsWith('/') || caret < 1) return null;
  const typed = raw.slice(1, caret);
  if (/\s/.test(typed)) return null;
  const word = raw.slice(1).split(/\s/, 1)[0] ?? '';
  if (!NAME_CHARS.test(word)) return null;
  return typed;
}

/**
 * The command whose argument is being typed and the argument so far, up to the
 * caret: `/mode de|` → { name: 'mode', query: 'de' }.
 */
export function argumentQuery(
  raw: string,
  caret: number,
): { name: string; query: string } | null {
  const match = /^\/([A-Za-z][A-Za-z0-9_:-]*(?:@[^\s/]+)?)\s+([\s\S]*)$/.exec(
    raw.slice(0, caret),
  );
  if (!match) return null;
  return { name: (match[1] ?? '').toLowerCase(), query: match[2] ?? '' };
}

function isSubsequence(text: string, query: string): boolean {
  let at = 0;
  for (const char of text) {
    if (char === query[at]) at += 1;
    if (at === query.length) return true;
  }
  return query.length === 0;
}

function commandScore(command: AgentCommand, query: string): number {
  const name = command.name.toLowerCase();
  if (name === query) return 5;
  if (name.startsWith(query)) return 4;
  if (name.includes(query)) return 3;
  if (isSubsequence(name, query)) return 2;
  if (command.description.toLowerCase().includes(query)) return 1;
  return 0;
}

/**
 * The commands that match what was typed of a name, best first: the name
 * itself, then names that start with it, contain it or contain its letters in
 * order, then descriptions that mention it. Ties keep the backend's order.
 */
export function filterCommands(commands: readonly AgentCommand[], query: string): AgentCommand[] {
  const wanted = query.trim().toLowerCase();
  if (!wanted) return [...commands];
  return commands
    .map((command, order) => ({ command, order, score: commandScore(command, wanted) }))
    .filter((entry) => entry.score > 0)
    .sort((a, b) => b.score - a.score || a.order - b.order)
    .map((entry) => entry.command);
}

const SOURCE_ORDER: readonly CommandSource[] = ['builtin', 'skill', 'mcp'];

/** Commands in groups by where they come from, in a fixed order, keeping their order inside a group. */
export function groupCommands(
  commands: readonly AgentCommand[],
): Array<{ source: CommandSource; commands: AgentCommand[] }> {
  return SOURCE_ORDER.map((source) => ({
    source,
    commands: commands.filter((command) => command.source === source),
  })).filter((group) => group.commands.length > 0);
}

/** One suggestion for a command's argument. */
export interface ArgumentOption {
  /** What goes after the command name. */
  value: string;
  /** What the option is called, when that is not the value itself. */
  label?: string;
  detail?: string;
}

/** The options that match the argument typed so far: those that start with it first. */
export function filterOptions(options: readonly ArgumentOption[], query: string): ArgumentOption[] {
  const wanted = query.trim().toLowerCase();
  if (!wanted) return [...options];
  const text = (option: ArgumentOption) => `${option.value} ${option.label ?? ''}`.toLowerCase();
  const starts = options.filter((option) => text(option).startsWith(wanted));
  const contains = options.filter(
    (option) => !text(option).startsWith(wanted) && text(option).includes(wanted),
  );
  return [...starts, ...contains];
}

/**
 * The text a chosen command leaves in the composer: its name and a space, so the
 * menu closes and what follows can be written at once; the text is sent trimmed.
 */
export function completeCommand(command: AgentCommand): string {
  return `/${command.name} `;
}

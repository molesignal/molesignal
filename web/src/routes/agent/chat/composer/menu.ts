import type { AgentCommand } from '@/api/agent/commands';

import {
  type ArgumentOption,
  argumentQuery,
  commandQuery,
  filterCommands,
  filterOptions,
} from './commands';

/** What the menu over the composer shows. */
export type CommandMenuState =
  /** Commands whose names match what is typed after the slash. */
  | { kind: 'commands'; query: string; commands: AgentCommand[] }
  /** Suggestions for the argument of a command. */
  | { kind: 'arguments'; command: AgentCommand; query: string; options: ArgumentOption[] }
  /** How the argument of a command is written, when there is nothing to pick from. */
  | { kind: 'hint'; command: AgentCommand };

/** The menu for the text and the caret, or none. */
export function commandMenu(
  input: string,
  caret: number,
  commands: readonly AgentCommand[],
  optionsFor: (command: AgentCommand) => readonly ArgumentOption[],
): CommandMenuState | null {
  const naming = commandQuery(input, caret);
  if (naming !== null) {
    const matches = filterCommands(commands, naming);
    return matches.length > 0 ? { kind: 'commands', query: naming, commands: matches } : null;
  }

  const argument = argumentQuery(input, caret);
  if (!argument) return null;
  const command = commands.find((item) => item.name.toLowerCase() === argument.name);
  if (!command?.argument_required) return null;

  const options = filterOptions(optionsFor(command), argument.query);
  if (options.length > 0) {
    return { kind: 'arguments', command, query: argument.query, options };
  }
  const freeText = command.choices.length === 0 && command.suggest === null;
  return freeText || !argument.query.trim() ? { kind: 'hint', command } : null;
}

/** How many rows of the menu can be chosen. */
export function menuLength(menu: CommandMenuState | null): number {
  if (!menu) return 0;
  if (menu.kind === 'commands') return menu.commands.length;
  if (menu.kind === 'arguments') return menu.options.length;
  return 0;
}

/** A key the menu changes its rows on: the highlight goes back to the first row when it changes. */
export function menuKey(menu: CommandMenuState | null): string {
  if (!menu) return '';
  if (menu.kind === 'commands') return `commands:${menu.query}`;
  if (menu.kind === 'arguments') return `arguments:${menu.command.name}:${menu.query}`;
  return `hint:${menu.command.name}`;
}

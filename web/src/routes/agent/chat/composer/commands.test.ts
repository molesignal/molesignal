import { describe, expect, it } from 'vitest';

import type { AgentCommand } from '@/api/agent/commands';

import {
  argumentQuery,
  commandQuery,
  completeCommand,
  displayText,
  filterCommands,
  filterOptions,
  findCommand,
  groupCommands,
  parseInput,
} from './commands';

function command(name: string, description: string, argument = false): AgentCommand {
  return {
    name,
    kind: 'control',
    source: 'builtin',
    description,
    argument_hint: argument ? '<x>' : null,
    argument_required: argument,
    choices: [],
    suggest: null,
    available: true,
    unavailable_reason: null,
  };
}

const COMMANDS = [
  command('help', 'List the commands'),
  command('mode', 'Switch the analysis mode', true),
  command('model', 'Choose the model', true),
  command('dashboard', 'Build a dashboard', true),
  command('export', 'Download this chat as Markdown'),
];

describe('slash command input', () => {
  it('reads a slash and a name as a command, the way the backend does', () => {
    expect(parseInput('/help')).toEqual({ kind: 'command', name: 'help', args: '' });
    expect(parseInput('  /Mode   deep ')).toEqual({ kind: 'command', name: 'mode', args: 'deep' });
    expect(parseInput('/query why is\ncheckout slow')).toEqual({
      kind: 'command',
      name: 'query',
      args: 'why is\ncheckout slow',
    });
    expect(parseInput('/github:review 42')).toEqual({
      kind: 'command',
      name: 'github:review',
      args: '42',
    });
  });

  it('reads a team after the name, the way the backend does', () => {
    expect(parseInput('/runbook@SRE值班 db down')).toEqual({
      kind: 'command',
      name: 'runbook',
      team: 'SRE值班',
      args: 'db down',
    });
    expect(parseInput('/runbook')).not.toHaveProperty('team');
  });

  it('keeps paths, bare slashes and other text as text', () => {
    for (const text of [
      '/var/log/app.log has errors',
      '/ is the root',
      '/',
      '/1h ago',
      '/帮助',
      'why /help',
      '/runbook@ x',
      '/a@b/c',
    ]) {
      expect(parseInput(text)).toEqual({ kind: 'text', text });
    }
  });

  it('finds the command a team skill goes by, ignoring case', () => {
    const commands = [
      { ...command('runbook', 'Our runbook'), source: 'skill' as const },
      { ...command('triage@SRE', 'Their triage'), source: 'skill' as const, team: 'SRE' },
    ];

    expect(findCommand(commands, { name: 'runbook' })?.description).toBe('Our runbook');
    expect(findCommand(commands, { name: 'triage', team: 'sre' })?.description).toBe('Their triage');
    expect(findCommand(commands, { name: 'triage' })).toBeUndefined();
  });

  it('sends an escaped slash as text without the backslash', () => {
    expect(parseInput('\\/help me')).toEqual({ kind: 'text', text: '/help me' });
    expect(displayText('  \\/query')).toBe('/query');
    expect(displayText('/query x')).toBe('/query x');
  });

  it('knows when a name is being typed and when its argument is', () => {
    expect(commandQuery('/', 1)).toBe('');
    expect(commandQuery('/mo', 3)).toBe('mo');
    expect(commandQuery('/mode', 2)).toBe('m');
    expect(commandQuery('/mode deep', 10)).toBeNull();
    expect(commandQuery('/var/log', 4)).toBeNull();
    expect(commandQuery('hello', 3)).toBeNull();
    expect(commandQuery('/mode', 0)).toBeNull();
    expect(commandQuery('/runbook@s', 10)).toBe('runbook@s');
    expect(commandQuery('/runbook@a/b', 10)).toBeNull();

    expect(argumentQuery('/mode de', 8)).toEqual({ name: 'mode', query: 'de' });
    expect(argumentQuery('/Model ', 7)).toEqual({ name: 'model', query: '' });
    expect(argumentQuery('/mode', 5)).toBeNull();
    expect(argumentQuery('mode deep', 9)).toBeNull();
    expect(argumentQuery('/runbook@SRE db', 15)).toEqual({ name: 'runbook@sre', query: 'db' });
  });

  it('puts names that start with the typed letters first, then any that contain them', () => {
    expect(filterCommands(COMMANDS, 'mo').map((item) => item.name)).toEqual(['mode', 'model']);
    expect(filterCommands(COMMANDS, 'ode').map((item) => item.name)).toEqual(['mode', 'model']);
    expect(filterCommands(COMMANDS, 'dsh').map((item) => item.name)).toEqual(['dashboard']);
    expect(filterCommands(COMMANDS, 'markdown').map((item) => item.name)).toEqual(['export']);
    expect(filterCommands(COMMANDS, '').map((item) => item.name)).toEqual(COMMANDS.map((item) => item.name));
    expect(filterCommands(COMMANDS, 'zzz')).toEqual([]);
  });

  it('groups commands by where they come from', () => {
    expect(groupCommands(COMMANDS)).toEqual([{ source: 'builtin', commands: COMMANDS }]);

    const skill = { ...command('runbook', 'Our runbook'), source: 'skill' as const };
    const prompt = { ...command('github:review', 'Review a PR'), source: 'mcp' as const };
    expect(groupCommands([prompt, skill, COMMANDS[0]!]).map((group) => group.source)).toEqual([
      'builtin',
      'skill',
      'mcp',
    ]);
  });

  it('offers the arguments that start with what is typed before those that only contain it', () => {
    const options = [
      { value: 'auto', label: 'Automatic' },
      { value: 'prod-openai', label: 'gpt-4o' },
      { value: 'openai', label: 'gpt-4o-mini' },
    ];
    expect(filterOptions(options, 'open').map((option) => option.value)).toEqual(['openai', 'prod-openai']);
    expect(filterOptions(options, 'GPT-4O-MINI').map((option) => option.value)).toEqual(['openai']);
    expect(filterOptions(options, '')).toHaveLength(3);
  });

  it('leaves a space after a chosen command, for what follows it', () => {
    expect(completeCommand(COMMANDS[1]!)).toBe('/mode ');
    expect(completeCommand(COMMANDS[0]!)).toBe('/help ');
  });
});

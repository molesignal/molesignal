import { describe, expect, it } from 'vitest';

import type { AgentCommand } from '@/api/agent/commands';

import { modeFor, pickByName, policyFor, timePresetFor } from './controlCommands';
import { commandMenu, menuKey, menuLength } from './menu';

function command(name: string, extra: Partial<AgentCommand> = {}): AgentCommand {
  return {
    name,
    kind: 'control',
    source: 'builtin',
    description: name,
    argument_hint: null,
    argument_required: false,
    choices: [],
    suggest: null,
    available: true,
    unavailable_reason: null,
    ...extra,
  };
}

const COMMANDS = [
  command('help'),
  command('mode', { argument_required: true, choices: ['auto', 'quick', 'deep', 'query'] }),
  command('query', { kind: 'task', argument_required: true, argument_hint: '<question>' }),
  command('rca', { kind: 'task', argument_required: true, suggest: 'incidents' }),
];

const optionsFor = (item: AgentCommand) =>
  item.name === 'rca'
    ? [{ value: 'inc-1', label: 'API answers with 502' }]
    : item.choices.map((value) => ({ value }));

describe('the menu over the composer', () => {
  it('lists the commands while a name is typed', () => {
    const menu = commandMenu('/', 1, COMMANDS, optionsFor);
    expect(menu?.kind).toBe('commands');
    expect(menuLength(menu)).toBe(4);
    expect(menuKey(commandMenu('/q', 2, COMMANDS, optionsFor))).toBe('commands:q');
    expect(commandMenu('/zzz', 4, COMMANDS, optionsFor)).toBeNull();
  });

  it('offers the words of a command once its name is complete', () => {
    const menu = commandMenu('/mode d', 7, COMMANDS, optionsFor);
    expect(menu?.kind).toBe('arguments');
    expect(menu?.kind === 'arguments' && menu.options.map((option) => option.value)).toEqual(['deep']);
    expect(commandMenu('/mode x', 7, COMMANDS, optionsFor)).toBeNull();
  });

  it('says how a free argument is written while it is typed', () => {
    expect(commandMenu('/query ', 7, COMMANDS, optionsFor)?.kind).toBe('hint');
    expect(commandMenu('/query errors by', 16, COMMANDS, optionsFor)?.kind).toBe('hint');
    expect(menuLength(commandMenu('/query ', 7, COMMANDS, optionsFor))).toBe(0);
  });

  it('suggests incidents for /rca and says how to write one when there are none', () => {
    expect(commandMenu('/rca ', 5, COMMANDS, optionsFor)?.kind).toBe('arguments');
    expect(commandMenu('/rca ', 5, COMMANDS, () => [])?.kind).toBe('hint');
    expect(commandMenu('/rca nope', 9, COMMANDS, () => [])).toBeNull();
  });

  it('shows nothing for a command without an argument or for plain text', () => {
    expect(commandMenu('/help ', 6, COMMANDS, optionsFor)).toBeNull();
    expect(commandMenu('why is it slow', 5, COMMANDS, optionsFor)).toBeNull();
  });
});

describe('control command words', () => {
  it('reads the words of /mode, /policy and /time', () => {
    expect(modeFor('Deep')).toBe('deep');
    expect(modeFor('query')).toBe('query_only');
    expect(modeFor('fast')).toBeNull();
    expect(policyFor('readonly')).toBe('read_only');
    expect(policyFor('approval')).toBe('policy');
    expect(timePresetFor(' 1H ')).toBe('1h');
    expect(timePresetFor('2h')).toBeNull();
  });

  it('picks by the first name before any other, then by id, and knows auto', () => {
    const providers = [
      { id: 'p1', name: 'Team OpenAI', model: 'gpt-4o' },
      { id: 'p2', name: 'gpt-4o', model: 'claude' },
    ];
    const names = (item: (typeof providers)[number]) => [item.name, item.model];
    expect(pickByName(providers, names, 'team openai')).toBe(providers[0]);
    expect(pickByName(providers, names, 'gpt-4o')).toBe(providers[1]);
    expect(pickByName(providers, names, 'claude')).toBe(providers[1]);
    expect(pickByName(providers, names, 'P1')).toBe(providers[0]);
    expect(pickByName(providers, names, 'AUTO')).toBe('auto');
    expect(pickByName(providers, names, 'nope')).toBeNull();
  });
});

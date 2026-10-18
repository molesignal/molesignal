/**
 * Mock slash commands of the Mole Agent chat: the list the backend gives the
 * composer, in English, and how it reads a message that is a command. A task
 * command is kept with the message; an unknown command, a control command, a
 * command without its argument and an incident that is not there are refused
 * with the backend's codes, and nothing is kept. Skills run as `/name` (a
 * team's not in the chat yet as `/name@team`), MCP prompts as
 * `/<server>:<prompt>`; a prompt of a server that takes each member's own
 * credential is refused until it is saved.
 */
import type { Express } from 'express';

import type { AgentCommand, MessageCommand } from '../../src/api/agent/commands';
import { parseInput } from '../../src/routes/agent/chat/composer/commands';

function command(
  name: string,
  kind: AgentCommand['kind'],
  description: string,
  argument: { hint?: string; choices?: string[]; suggest?: AgentCommand['suggest'] } | null,
): AgentCommand {
  return {
    name,
    kind,
    source: 'builtin',
    description,
    argument_hint: argument ? (argument.choices?.join(' | ') ?? argument.hint ?? null) : null,
    argument_required: argument !== null,
    choices: argument?.choices ?? [],
    suggest: argument?.suggest ?? null,
    available: true,
    unavailable_reason: null,
  };
}

export const MOCK_AGENT_COMMANDS: AgentCommand[] = [
  command('help', 'control', 'List the commands', null),
  command('new', 'control', 'Start a new chat', null),
  command('team', 'control', 'Make this a team chat', { hint: '<team>', suggest: 'teams' }),
  command('mode', 'control', 'Switch the analysis mode', { choices: ['auto', 'quick', 'deep', 'query'] }),
  command('time', 'control', 'Set the time range', { choices: ['15m', '30m', '1h', '6h', '24h', '7d'] }),
  command('policy', 'control', 'Set what Mole Agent may run', { choices: ['advice', 'readonly', 'approval', 'auto', 'bypass'] }),
  command('model', 'control', 'Choose the model', { hint: '<model provider>', suggest: 'models' }),
  command('profile', 'control', 'Choose the agent profile', { hint: '<agent profile>', suggest: 'profiles' }),
  command('prompt', 'control', 'Choose the prompt', { hint: '<prompt>', suggest: 'prompts' }),
  command('query', 'task', 'Write a query without running it', { hint: '<question>' }),
  command('dashboard', 'task', 'Build a dashboard', { hint: '<what it should show>' }),
  command('rca', 'task', 'Investigate the root cause of an incident', { hint: '<incident>', suggest: 'incidents' }),
  command('export', 'control', 'Download this chat as Markdown', null),
  {
    ...command('incident-triage', 'task', 'Confirm what is broken and who owns it', null),
    source: 'skill',
  },
  {
    ...command('runbook@Platform', 'task', 'Follow the platform team runbook', null),
    source: 'skill',
    team: 'Platform',
  },
  {
    ...command('linear:triage', 'task', 'Triage a Linear issue', { hint: '<issue>' }),
    source: 'mcp',
    server: 'Linear',
    available: false,
    unavailable_reason: 'Add your own credential for Linear first',
  },
  {
    ...command('github:review_pr', 'task', 'Review a pull request', { hint: '<pr> [focus]' }),
    source: 'mcp',
    server: 'GitHub',
  },
];

export interface MockAgentCommands {
  /**
   * What the backend makes of a message: the command to keep with it, or the
   * refusal to answer with.
   */
  read(content: string): { command: MessageCommand | null } | { status: number; body: Record<string, unknown> };
}

export function registerAgentCommandRoutes(
  app: Express,
  incidentExists: (id: string) => boolean,
): MockAgentCommands {
  app.get('/api/v1/agent/commands', (_req, res) => res.json({ commands: MOCK_AGENT_COMMANDS }));

  const refuse = (status: number, code: string, name: string) => ({
    status,
    body: { error: code, code, command: name, message: `/${name}: ${code}` },
  });

  return {
    read: (content) => {
      const parsed = parseInput(content);
      if (parsed.kind === 'text') return { command: null };
      const shown = parsed.team ? `${parsed.name}@${parsed.team}` : parsed.name;
      const known = MOCK_AGENT_COMMANDS.find(
        (item) => item.name.toLowerCase() === shown.toLowerCase(),
      );
      if (!known) return refuse(400, 'unknown_command', shown);
      if (known.source === 'skill') {
        return { command: { name: shown, args: parsed.args, source: 'skill' } };
      }
      if (known.source === 'mcp') {
        if (!known.available) return refuse(403, 'command_needs_credential', shown);
        if (!parsed.args) return refuse(400, 'command_argument_missing', shown);
        return { command: { name: shown, args: parsed.args, source: 'mcp' } };
      }
      if (known.kind === 'control') return refuse(400, 'client_command', parsed.name);
      if (!parsed.args) return refuse(400, 'command_argument_missing', parsed.name);
      if (parsed.name === 'rca' && !incidentExists(parsed.args.split(/\s/)[0] ?? '')) {
        return refuse(404, 'command_target_not_found', parsed.name);
      }
      return { command: { name: parsed.name, args: parsed.args, source: 'builtin' } };
    },
  };
}

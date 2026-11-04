import type { ImBot, ImBotStatus } from '@/api/agent/im';

/** What a bot's row says about it, worst first: off, missing settings, then the connection. */
export type ImBotState =
  | { kind: 'missing'; keys: string[] }
  | { kind: 'connector_off' }
  | { kind: 'off' }
  | { kind: ImBotStatus; detail: string | null };

export function botState(bot: ImBot): ImBotState {
  if (bot.missing.length > 0) return { kind: 'missing', keys: bot.missing };
  if (!bot.connectorEnabled) return { kind: 'connector_off' };
  if (!bot.enabled) return { kind: 'off' };
  return { kind: bot.status, detail: bot.statusDetail };
}

/** The ink a state is written in: green when connected, red when it cannot answer. */
export function stateTone(state: ImBotState): 'good' | 'bad' | 'muted' {
  switch (state.kind) {
    case 'connected':
      return 'good';
    case 'error':
    case 'missing':
    case 'connector_off':
      return 'bad';
    default:
      return 'muted';
  }
}

/** The team a group's new threads go to, as one value for a select: `personal` or the team ID. */
export const PERSONAL = 'personal';

export function teamChoice(bot: Pick<ImBot, 'defaultTeamId'>): string {
  return bot.defaultTeamId ?? PERSONAL;
}

export function teamOfChoice(choice: string): string | null {
  return choice === PERSONAL ? null : choice;
}

/** Where the docs describe setting the platforms up, in the reader's language. */
export function imDocumentationUrl(language: string | undefined): string {
  const locale = language?.toLowerCase().startsWith('zh') ? 'zh-Hans' : 'en-US';
  return `https://docs.molesignal.com/${locale}/im`;
}

import type { TFunction } from 'i18next';

import type {
  Automation,
  AutomationChannel,
  AutomationInput,
  AutomationRun,
  AutomationRunStatus,
  AutomationTemplate,
  AutomationTrigger,
  AutomationTriggerKind,
} from '@/api/agent/automations';

export const NAME_MAX = 120;
export const PROMPT_MAX = 8_000;
export const DESCRIPTION_MAX = 1_000;
export const MAX_CHANNELS = 5;

/** How long a run may take, in seconds: five minutes to an hour. */
export const TIMEOUT_CHOICES = [300, 600, 1_200, 1_800, 3_600] as const;
export const DEFAULT_TIMEOUT = 600;

export const TRIGGER_KINDS: readonly AutomationTriggerKind[] = ['schedule', 'incident', 'finding', 'api', 'manual'];
export const SEVERITIES = ['critical', 'error', 'warning', 'info'] as const;

/** Common schedules, offered next to the cron field. */
export const CRON_PRESETS = [
  { key: 'weekdays_morning', cron: '0 9 * * 1-5' },
  { key: 'daily_morning', cron: '0 9 * * *' },
  { key: 'monday_morning', cron: '0 10 * * 1' },
  { key: 'hourly', cron: '0 * * * *' },
] as const;

/** What the editor holds while it is open. */
export interface AutomationDraft {
  name: string;
  description: string;
  prompt: string;
  enabled: boolean;
  kind: AutomationTriggerKind;
  cron: string;
  timezone: string;
  severities: string[];
  ruleIds: string[];
  services: string[];
  /** `key=value` per line. */
  labels: string;
  /** Finding triggers: the kinds, the least severity (empty: any) and the subjects. */
  findingKinds: string[];
  minSeverity: string;
  subjects: string[];
  channels: AutomationChannel[];
  writeRca: boolean;
  timeoutSecs: number;
  /** New ones only: `personal`, `org` or `team:<id>`. */
  scope: string;
  template: string | null;
}

export function newDraft(timezone: string): AutomationDraft {
  return {
    name: '',
    description: '',
    prompt: '',
    enabled: true,
    kind: 'schedule',
    cron: CRON_PRESETS[0].cron,
    timezone,
    severities: ['critical', 'error'],
    ruleIds: [],
    services: [],
    labels: '',
    findingKinds: [],
    minSeverity: 'warning',
    subjects: [],
    channels: [],
    writeRca: false,
    timeoutSecs: DEFAULT_TIMEOUT,
    scope: 'personal',
    template: null,
  };
}

function withTrigger(draft: AutomationDraft, trigger: AutomationTrigger): AutomationDraft {
  switch (trigger.kind) {
    case 'schedule':
      return { ...draft, kind: 'schedule', cron: trigger.cron, timezone: trigger.timezone };
    case 'incident':
      return {
        ...draft,
        kind: 'incident',
        severities: trigger.severities ?? [],
        ruleIds: trigger.ruleIds ?? [],
        services: trigger.services ?? [],
        labels: Object.entries(trigger.labels ?? {})
          .map(([key, value]) => `${key}=${value}`)
          .join('\n'),
      };
    case 'finding':
      return {
        ...draft,
        kind: 'finding',
        findingKinds: trigger.kinds ?? [],
        minSeverity: trigger.minSeverity ?? '',
        subjects: trigger.subjects ?? [],
      };
    default:
      return { ...draft, kind: trigger.kind };
  }
}

export function draftFromAutomation(automation: Automation, timezone: string): AutomationDraft {
  return withTrigger(
    {
      ...newDraft(timezone),
      name: automation.name,
      description: automation.description,
      prompt: automation.prompt,
      enabled: automation.enabled,
      channels: automation.outputs.channels,
      writeRca: automation.outputs.writeRca,
      timeoutSecs: automation.timeoutSecs,
      template: automation.template ?? null,
    },
    automation.trigger,
  );
}

export function draftFromTemplate(template: AutomationTemplate, timezone: string): AutomationDraft {
  return withTrigger(
    {
      ...newDraft(timezone),
      name: template.name,
      description: template.description,
      prompt: template.prompt,
      writeRca: template.outputs.writeRca,
      template: template.key,
    },
    template.trigger,
  );
}

/** A copy to save in another scope: the same task, a new name. */
export function draftForClone(automation: Automation, timezone: string, name: string): AutomationDraft {
  return { ...draftFromAutomation(automation, timezone), name, enabled: false, template: null };
}

/** `key=value` lines as labels; lines without `=` are left out. */
export function parseLabels(text: string): Record<string, string> {
  const labels: Record<string, string> = {};
  for (const line of text.split('\n')) {
    const at = line.indexOf('=');
    if (at <= 0) continue;
    const key = line.slice(0, at).trim();
    if (key) labels[key] = line.slice(at + 1).trim();
  }
  return labels;
}

export function triggerOf(draft: AutomationDraft): AutomationTrigger {
  switch (draft.kind) {
    case 'schedule':
      return { kind: 'schedule', cron: draft.cron.trim(), timezone: draft.timezone };
    case 'incident':
      return {
        kind: 'incident',
        severities: draft.severities,
        ruleIds: draft.ruleIds,
        services: draft.services,
        labels: parseLabels(draft.labels),
      };
    case 'finding':
      return {
        kind: 'finding',
        kinds: draft.findingKinds,
        minSeverity: draft.minSeverity || null,
        subjects: draft.subjects,
      };
    case 'api':
      return { kind: 'api' };
    case 'manual':
      return { kind: 'manual' };
  }
}

export function inputFromDraft(draft: AutomationDraft, creating: boolean): AutomationInput {
  const input: AutomationInput = {
    name: draft.name.trim(),
    description: draft.description.trim(),
    prompt: draft.prompt.trim(),
    enabled: draft.enabled,
    trigger: triggerOf(draft),
    outputs: {
      channels: draft.channels.map((channel) => ({ ...channel, target: channel.target.trim() })),
      writeRca: draft.kind === 'incident' && draft.writeRca,
    },
    timeoutSecs: draft.timeoutSecs,
  };
  if (!creating) return input;

  const [scope, teamId] = draft.scope.startsWith('team:')
    ? (['team', draft.scope.slice('team:'.length)] as const)
    : ([draft.scope === 'org' ? 'org' : 'personal', undefined] as const);
  return {
    ...input,
    scope,
    ...(teamId ? { teamId: teamId } : {}),
    template: draft.template,
  };
}

/** Five fields, each made of what cron accepts. The server checks the rest, and the 15 minutes. */
export function cronLooksValid(cron: string): boolean {
  const fields = cron.trim().split(/\s+/);
  return fields.length === 5 && fields.every((field) => /^[\d*,/\-A-Za-z?]+$/.test(field));
}

export type DraftProblem = 'name' | 'prompt' | 'cron' | 'channel';

export function draftProblems(draft: AutomationDraft): DraftProblem[] {
  const problems: DraftProblem[] = [];
  if (!draft.name.trim() || draft.name.trim().length > NAME_MAX) problems.push('name');
  if (!draft.prompt.trim() || draft.prompt.trim().length > PROMPT_MAX) problems.push('prompt');
  if (draft.kind === 'schedule' && !cronLooksValid(draft.cron)) problems.push('cron');
  if (draft.channels.some((channel) => !channel.target.trim())) problems.push('channel');
  return problems;
}

/** What starts it, in a few words for the list. */
export function triggerSummary(t: TFunction, trigger: AutomationTrigger): string {
  switch (trigger.kind) {
    case 'schedule': {
      const preset = CRON_PRESETS.find((candidate) => candidate.cron === trigger.cron);
      return preset ? t(`automations.cron.${preset.key}`) : trigger.cron;
    }
    case 'incident':
      return trigger.severities.length > 0
        ? t('automations.trigger_summary.incident_severities', {
            severities: trigger.severities
              .map((severity) => t(`automations.severity.${severity}`, { defaultValue: severity }))
              .join(t('automations.list_separator')),
          })
        : t('automations.trigger_summary.incident');
    case 'finding':
      return trigger.minSeverity
        ? t('automations.trigger_summary.finding_severity', {
            severity: t(`automations.severity.${trigger.minSeverity}`, { defaultValue: trigger.minSeverity }),
          })
        : t('automations.trigger_summary.finding');
    case 'api':
      return t('automations.trigger_summary.api');
    case 'manual':
      return t('automations.trigger_summary.manual');
  }
}

/** How a run ended, as a tone for its word. */
export function runTone(status: AutomationRunStatus): 'green' | 'orange' | 'red' | 'blue' | 'dim' {
  switch (status) {
    case 'succeeded':
      return 'green';
    case 'partial':
    case 'blocked':
      return 'orange';
    case 'failed':
      return 'red';
    case 'running':
      return 'blue';
    default:
      return 'dim';
  }
}

/** The reason codes of a run, in order. */
export function reasonsOf(run: Pick<AutomationRun, 'reason'>): string[] {
  return (run.reason ?? '')
    .split(',')
    .map((reason) => reason.trim())
    .filter(Boolean);
}

/** How a caller starts it: the request a CI/CD job sends. */
export function curlExample(origin: string, path: string, token: string): string {
  return [
    `curl -X POST '${origin.replace(/\/$/, '')}${path}' \\`,
    `  -H 'Authorization: Bearer ${token}' \\`,
    `  -H 'Content-Type: application/json' \\`,
    `  -d '{"text": "Deployed checkout v42 at 14:05"}'`,
  ].join('\n');
}

export function automationTriggerPath(id: string): string {
  return `/api/v1/public/automations/${encodeURIComponent(id)}/trigger`;
}

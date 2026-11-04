import type { TFunction } from 'i18next';
import { describe, expect, it } from 'vitest';

import type { Automation, AutomationTemplate } from '@/api/agent/automations';

import {
  cronLooksValid,
  curlExample,
  draftForClone,
  draftFromAutomation,
  draftFromTemplate,
  draftProblems,
  inputFromDraft,
  newDraft,
  parseLabels,
  reasonsOf,
  triggerSummary,
} from './model';

const t = ((key: string, options?: Record<string, unknown>) =>
  options ? `${key} ${JSON.stringify(options)}` : key) as unknown as TFunction;

const incident: Automation = {
  id: 'automation-1',
  scope: { scope: 'team', teamId: 'team-sre' },
  name: 'Incident analysis',
  description: 'For the SRE team',
  prompt: 'Find the root cause.',
  enabled: true,
  trigger: {
    kind: 'incident',
    severities: ['critical'],
    ruleIds: ['rule-1'],
    services: ['checkout'],
    labels: { env: 'prod', region: 'eu' },
  },
  outputs: { channels: [], writeRca: true },
  runAsUserId: 'user-1',
  language: 'en-us',
  timeoutSecs: 1_200,
  createdBy: 'user-1',
  updatedBy: 'user-1',
  createdAt: 1,
  updatedAt: 1,
  editable: true,
  moderatable: true,
};

describe('an automation in the editor', () => {
  it('reads back what it was saved with', () => {
    const draft = draftFromAutomation(incident, 'UTC');

    expect(draft.kind).toBe('incident');
    expect(draft.labels).toBe('env=prod\nregion=eu');
    expect(inputFromDraft(draft, false)).toEqual({
      name: 'Incident analysis',
      description: 'For the SRE team',
      prompt: 'Find the root cause.',
      enabled: true,
      trigger: {
        kind: 'incident',
        severities: ['critical'],
        ruleIds: ['rule-1'],
        services: ['checkout'],
        labels: { env: 'prod', region: 'eu' },
      },
      outputs: { channels: [], writeRca: true },
      timeoutSecs: 1_200,
    });
  });

  it('says whose a new one is, and only an incident automation writes an RCA', () => {
    const draft = { ...newDraft('Asia/Shanghai'), name: 'Daily', prompt: 'Check.', scope: 'team:team-sre', writeRca: true };
    const input = inputFromDraft(draft, true);

    expect(input).toMatchObject({ scope: 'team', teamId: 'team-sre', template: null });
    expect(input.outputs.writeRca).toBe(false);
    expect(input.trigger).toEqual({ kind: 'schedule', cron: '0 9 * * 1-5', timezone: 'Asia/Shanghai' });
  });

  it('starts from a template in the reader’s time zone, and a clone starts switched off', () => {
    const template: AutomationTemplate = {
      key: 'daily_health',
      name: 'Daily health check',
      description: 'Every weekday',
      prompt: 'Check the health.',
      trigger: { kind: 'schedule', cron: '0 9 * * 1-5', timezone: 'Europe/Berlin' },
      outputs: { channels: [], writeRca: false },
    };

    const draft = draftFromTemplate(template, 'UTC');
    expect(draft).toMatchObject({ name: 'Daily health check', template: 'daily_health', timezone: 'Europe/Berlin' });

    const clone = draftForClone(incident, 'UTC', 'Incident analysis (copy)');
    expect(clone).toMatchObject({ name: 'Incident analysis (copy)', enabled: false, template: null });
  });

  it('holds back a draft without a name, a task or a readable cron', () => {
    expect(draftProblems(newDraft('UTC'))).toEqual(['name', 'prompt']);
    expect(draftProblems({ ...newDraft('UTC'), name: 'A', prompt: 'B', cron: '0 9 * *' })).toEqual(['cron']);
    expect(cronLooksValid('*/15 * * * *')).toBe(true);
    expect(cronLooksValid('0 9 * * MON-FRI')).toBe(true);
    expect(cronLooksValid('0 9 * * 1; rm')).toBe(false);
  });
});

describe('an automation in the list', () => {
  it('names a preset schedule, the severities of an incident trigger, and an API call', () => {
    expect(triggerSummary(t, { kind: 'schedule', cron: '0 9 * * 1-5', timezone: 'UTC' })).toBe(
      'automations.cron.weekdays_morning',
    );
    expect(triggerSummary(t, { kind: 'schedule', cron: '5 4 * * *', timezone: 'UTC' })).toBe('5 4 * * *');
    expect(triggerSummary(t, incident.trigger)).toContain('automations.trigger_summary.incident_severities');
    expect(triggerSummary(t, { kind: 'api' })).toBe('automations.trigger_summary.api');
  });

  it('reads labels, reasons and the call a CI/CD job sends', () => {
    expect(parseLabels('env = prod\nbroken\n=x\nteam=sre')).toEqual({ env: 'prod', team: 'sre' });
    expect(reasonsOf({ reason: 'credential, delivery' })).toEqual(['credential', 'delivery']);
    expect(reasonsOf({ reason: null })).toEqual([]);
    expect(curlExample('https://ms.example/', '/api/v1/public/automations/a1/trigger', 'mspat-x')).toContain(
      "curl -X POST 'https://ms.example/api/v1/public/automations/a1/trigger'",
    );
  });
});

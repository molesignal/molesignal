/**
 * Mock Mole Agent automations: three rules (a daily health check of the
 * platform team, the organization's incident analysis, and a change check a
 * CI/CD job calls), their runs, the built-in templates, manual runs and API
 * tokens, the way `/api/v1/agent/automations` answers. Kept per test.
 */
import type { Express } from 'express';

import type { Automation, AutomationRun, AutomationTemplate } from '../../src/api/agent/automations';

const MINUTE = 60_000_000;

/** What every mock automation shares: run as the mock user, who may change it. */
function baseOf(now: number) {
  return {
    runAsUserId: 'dev',
    language: 'en-us',
    timeoutSecs: 600,
    template: null,
    apiTokenPrefix: null,
    createdBy: 'dev',
    updatedBy: 'dev',
    createdAt: now - 7 * 24 * 60 * MINUTE,
    updatedAt: now - 60 * MINUTE,
    editable: true,
    moderatable: true,
  };
}

function seedAutomations(now: number): Automation[] {
  const base = baseOf(now);

  return [
    {
      ...base,
      id: 'automation-daily-health',
      scope: { scope: 'team', teamId: 'team-platform' },
      name: 'Daily health check',
      description: 'Errors, latency and open incidents of the checkout services',
      prompt: 'Check the health of the checkout services over the last 24 hours and say what needs attention.',
      enabled: true,
      trigger: { kind: 'schedule', cron: '0 9 * * 1-5', timezone: 'UTC' },
      outputs: {
        channels: [{ connectorId: 'connector-slack', targetType: 'fixed_group', target: '#sre' }],
        writeRca: false,
      },
      template: 'daily_health',
      nextRunAt: now + 14 * 60 * MINUTE,
      lastRun: { id: 'run-health-1', at: now - 10 * 60 * MINUTE, status: 'succeeded' },
    },
    {
      ...base,
      id: 'automation-incident-analysis',
      scope: { scope: 'org' },
      name: 'Incident analysis',
      description: 'Investigates new error and critical incidents',
      prompt: 'Investigate the incident attached to this message and find its root cause.',
      enabled: true,
      trigger: { kind: 'incident', severities: ['critical', 'error'], ruleIds: [], services: [], labels: {} },
      outputs: { channels: [], writeRca: true },
      template: 'incident_analysis',
      nextRunAt: null,
      lastRun: { id: 'run-incident-1', at: now - 35 * MINUTE, status: 'blocked' },
    },
    {
      ...base,
      id: 'automation-change-impact',
      scope: { scope: 'personal', ownerId: 'dev' },
      name: 'Change impact check',
      description: 'Called by the deploy pipeline',
      prompt: 'A change was just deployed; check whether it caused a regression.',
      enabled: false,
      trigger: { kind: 'api' },
      outputs: { channels: [], writeRca: false },
      template: 'change_impact',
      apiTokenPrefix: 'mspat-x9qe2k7m2q',
      nextRunAt: null,
      lastRun: null,
    },
  ];
}

function seedRuns(now: number): AutomationRun[] {
  const run = (overrides: Partial<AutomationRun> & Pick<AutomationRun, 'id' | 'automationId'>): AutomationRun => ({
    trigger: 'schedule',
    status: 'succeeded',
    reason: null,
    chatId: null,
    runId: null,
    incidentId: null,
    input: null,
    summary: null,
    outputs: {},
    startedBy: null,
    createdAt: now,
    finishedAt: now,
    ...overrides,
  });

  return [
    run({
      id: 'run-health-1',
      automationId: 'automation-daily-health',
      chatId: 'chat-automation-health-1',
      summary: 'Healthy, except checkout-api: p95 latency rose from 180 ms to 420 ms after 08:10.',
      outputs: {
        deliveries: [
          { connectorId: 'connector-slack', targetType: 'fixed_group', targetMasked: '#s**', delivered: true },
        ],
      },
      createdAt: now - 10 * 60 * MINUTE,
      finishedAt: now - 10 * 60 * MINUTE + 2 * MINUTE,
    }),
    run({
      id: 'run-health-0',
      automationId: 'automation-daily-health',
      status: 'failed',
      reason: 'timeout',
      createdAt: now - 34 * 60 * MINUTE,
      finishedAt: now - 34 * 60 * MINUTE + 10 * MINUTE,
    }),
    run({
      id: 'run-incident-1',
      automationId: 'automation-incident-analysis',
      trigger: 'incident',
      status: 'blocked',
      reason: 'credential',
      chatId: 'chat-automation-incident-1',
      incidentId: 'incident-checkout-502',
      input: 'checkout-api 5xx above 5%',
      summary: 'The 502s start with the 14:02 deploy of checkout-api; Linear could not be read without a credential.',
      outputs: { rca: true },
      createdAt: now - 35 * MINUTE,
      finishedAt: now - 33 * MINUTE,
    }),
  ];
}

const TEMPLATES: AutomationTemplate[] = [
  {
    key: 'incident_analysis',
    name: 'Incident analysis',
    description: "Investigates every new error or critical incident with tools and writes the conclusion to the incident's RCA.",
    prompt: 'Investigate the incident attached to this message and find its root cause.',
    trigger: { kind: 'incident', severities: ['error', 'critical'], ruleIds: [], services: [], labels: {} },
    outputs: { channels: [], writeRca: true },
  },
  {
    key: 'daily_health',
    name: 'Daily health check',
    description: 'Every weekday at 09:00, checks errors, latency and open incidents across services.',
    prompt: 'Check the health of our services over the last 24 hours.',
    trigger: { kind: 'schedule', cron: '0 9 * * 1-5', timezone: 'UTC' },
    outputs: { channels: [], writeRca: false },
  },
  {
    key: 'change_impact',
    name: 'Change impact check',
    description: 'Called by CI/CD after a deploy: compares the services before and after the change.',
    prompt: 'A change was just deployed; check whether it caused a regression.',
    trigger: { kind: 'api' },
    outputs: { channels: [], writeRca: false },
  },
];

export function registerAutomationRoutes(app: Express, now: number) {
  const automations = seedAutomations(now);
  const runs = seedRuns(now);
  let counter = 0;

  const find = (id: string) => automations.find((automation) => automation.id === id);

  app.get('/api/v1/agent/automations', (_req, res) => res.json({ automations, canCreate: true }));

  app.get('/api/v1/agent/automation-templates', (req, res) => {
    const timezone = String(req.query.timezone ?? 'UTC');
    res.json({
      templates: TEMPLATES.map((template) =>
        template.trigger.kind === 'schedule'
          ? { ...template, trigger: { ...template.trigger, timezone } }
          : template,
      ),
    });
  });

  app.post('/api/v1/agent/automations', (req, res) => {
    const body = req.body ?? {};
    const scope = (
      body.scope === 'team'
        ? { scope: 'team', teamId: body.teamId }
        : body.scope === 'org'
          ? { scope: 'org' }
          : { scope: 'personal', ownerId: 'dev' }
    ) as Automation['scope'];
    const created: Automation = {
      ...baseOf(now),
      id: `automation-new-${++counter}`,
      scope,
      name: body.name,
      description: body.description ?? '',
      prompt: body.prompt,
      enabled: body.enabled ?? true,
      trigger: body.trigger,
      outputs: body.outputs,
      timeoutSecs: body.timeoutSecs ?? 600,
      template: body.template ?? null,
      apiTokenPrefix: null,
      nextRunAt: body.trigger?.kind === 'schedule' && body.enabled ? now + 60 * MINUTE : null,
      lastRun: null,
      createdAt: now,
      updatedAt: now,
    };
    automations.push(created);
    res.status(201).json(created);
  });

  app.put('/api/v1/agent/automations/:id', (req, res) => {
    const automation = find(req.params.id);
    if (!automation) return res.status(404).json({ error: 'not_found', message: 'automation not found' });
    Object.assign(automation, {
      name: req.body?.name,
      description: req.body?.description ?? '',
      prompt: req.body?.prompt,
      enabled: req.body?.enabled,
      trigger: req.body?.trigger,
      outputs: req.body?.outputs,
      timeoutSecs: req.body?.timeoutSecs,
      updatedAt: now,
    });
    return res.json(automation);
  });

  app.delete('/api/v1/agent/automations/:id', (req, res) => {
    const at = automations.findIndex((automation) => automation.id === req.params.id);
    if (at >= 0) automations.splice(at, 1);
    res.json({ deleted: at >= 0 });
  });

  app.post('/api/v1/agent/automations/:id/enabled', (req, res) => {
    const automation = find(req.params.id);
    if (!automation) return res.status(404).json({ error: 'not_found', message: 'automation not found' });
    automation.enabled = Boolean(req.body?.enabled);
    return res.json(automation);
  });

  app.post('/api/v1/agent/automations/:id/run', (req, res) => {
    const automation = find(req.params.id);
    if (!automation) return res.status(404).json({ error: 'not_found', message: 'automation not found' });
    const started: AutomationRun = {
      id: `run-manual-${++counter}`,
      automationId: automation.id,
      trigger: 'manual',
      status: 'running',
      reason: null,
      chatId: `chat-automation-manual-${counter}`,
      runId: `agent-run-${counter}`,
      incidentId: null,
      input: null,
      summary: null,
      outputs: {},
      startedBy: 'dev',
      createdAt: Date.now() * 1000,
      finishedAt: null,
    };
    runs.unshift(started);
    automation.lastRun = { id: started.id, at: started.createdAt, status: 'running' };
    // The run answers a moment later, as a worker would.
    setTimeout(() => {
      started.status = 'succeeded';
      started.summary = 'All checkout services are healthy.';
      started.finishedAt = Date.now() * 1000;
      if (automation.lastRun?.id === started.id) automation.lastRun.status = 'succeeded';
    }, 800);
    return res.status(202).json({ run: started });
  });

  app.get('/api/v1/agent/automations/:id/runs', (req, res) =>
    res.json({ runs: runs.filter((run) => run.automationId === req.params.id) }),
  );

  app.post('/api/v1/agent/automations/:id/token', (req, res) => {
    const automation = find(req.params.id);
    if (!automation) return res.status(404).json({ error: 'not_found', message: 'automation not found' });
    const token = `mspat-mocktoken${++counter}abcdefghjkmnpqrstvwxyz0123`;
    automation.apiTokenPrefix = token.slice(0, 16);
    return res.json({
      token,
      prefix: automation.apiTokenPrefix,
      triggerPath: `/api/v1/public/automations/${automation.id}/trigger`,
    });
  });

  app.delete('/api/v1/agent/automations/:id/token', (req, res) => {
    const automation = find(req.params.id);
    if (automation) automation.apiTokenPrefix = null;
    res.json({ revoked: true });
  });
}

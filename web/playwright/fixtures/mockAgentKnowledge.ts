/**
 * Mock Mole Agent knowledge, memory, @ references and the start of an
 * investigation from an incident.
 *
 * The organization's knowledge has MOLE.md, services.md and a runbook changed
 * after MOLE.md (so the entry file may be out of date); the Platform team's is
 * empty and Payments' has one file. The signed-in `dev` user may change all of
 * them (they hold agent.manage). Their own memory holds a preference and a
 * fact; memory distillation is off. Kept per test.
 */
import { Buffer } from 'node:buffer';

import express, { type Express } from 'express';

import type { KnowledgeFile } from '../../src/api/agent/knowledge';
import type { Memory } from '../../src/api/agent/memory';

const MINUTE = 60_000_000;

export function registerAgentKnowledgeRoutes(app: Express, nowIso: string) {
  const now = Date.parse(nowIso) * 1000;
  const file = (path: string, content: string, ago: number): KnowledgeFile => ({
    path,
    content,
    size_bytes: Buffer.byteLength(content),
    updated_by: 'dev',
    updated_at_micros: now - ago,
  });
  const bases = new Map<string, KnowledgeFile[]>([
    [
      'org',
      [
        file('MOLE.md', '# Acme\n\n- @services.md - what runs where\n- @runbooks/api-5xx.md - when the API answers 5xx\n', 90 * MINUTE),
        file('services.md', '# Services\n\n| Service | Owner |\n|---|---|\n| checkout | Payments |\n', 90 * MINUTE),
        file('runbooks/api-5xx.md', '# API 5xx\n\n1. Check the load balancer.\n2. Roll back the last deploy.\n', 10 * MINUTE),
      ],
    ],
    ['team:team-platform', []],
    ['team:team-payments', [file('runbooks/declines.md', '# Card declines\n', 30 * MINUTE)]],
  ]);
  const keyOf = (scope: unknown, teamId: unknown) =>
    scope === 'team' ? `team:${String(teamId ?? '')}` : 'org';
  const teams: Record<string, string> = { 'team-platform': 'Platform', 'team-payments': 'Payments' };

  app.get('/api/v1/agent/knowledge', (_req, res) => {
    const summary = (key: string) => {
      const files = bases.get(key) ?? [];
      const entry = files.find((item) => item.path === 'MOLE.md');
      return {
        scope: key === 'org' ? 'org' : 'team',
        team_id: key === 'org' ? null : key.slice('team:'.length),
        team_name: key === 'org' ? null : teams[key.slice('team:'.length)],
        files: files.length,
        bytes: files.reduce((total, item) => total + item.size_bytes, 0),
        entry: Boolean(entry),
        entry_updated_at_micros: entry?.updated_at_micros ?? null,
        changed_since_entry: entry
          ? files.filter((item) => item.path !== 'MOLE.md' && item.updated_at_micros > entry.updated_at_micros).length
          : 0,
        updated_at_micros: files.length ? Math.max(...files.map((item) => item.updated_at_micros)) : null,
        can_edit: true,
      };
    };
    res.json({
      bases: ['org', 'team:team-payments', 'team:team-platform'].map(summary),
      entry_file: 'MOLE.md',
      limits: { max_file_bytes: 1_048_576, max_total_bytes: 5_242_880, max_files: 200 },
    });
  });

  app.get('/api/v1/agent/knowledge/files', (req, res) => {
    const files = bases.get(keyOf(req.query.scope, req.query.team_id)) ?? [];
    res.json({
      files: files.map(({ content: _content, ...meta }) => meta),
      can_edit: true,
    });
  });

  app.get('/api/v1/agent/knowledge/file', (req, res) => {
    const files = bases.get(keyOf(req.query.scope, req.query.team_id)) ?? [];
    const found = files.find((item) => item.path === req.query.path);
    if (!found) {
      res.status(404).json({ error: 'not_found', message: 'knowledge file not found' });
      return;
    }
    res.json(found);
  });

  app.put('/api/v1/agent/knowledge/file', (req, res) => {
    const key = keyOf(req.body?.scope, req.body?.team_id);
    const files = bases.get(key) ?? [];
    const path = String(req.body?.path ?? '').replace(/^\/+/, '');
    if (!path || path.split('/').some((part) => part.startsWith('.') || !part)) {
      res.status(400).json({ error: 'invalid', message: `\`${path}\` is not a knowledge path` });
      return;
    }
    const next = file(path, String(req.body?.content ?? ''), 0);
    bases.set(key, [...files.filter((item) => item.path !== path), next]);
    res.json({ written: true });
  });

  app.delete('/api/v1/agent/knowledge/file', (req, res) => {
    const key = keyOf(req.query.scope, req.query.team_id);
    const path = String(req.query.path ?? '');
    const files = bases.get(key) ?? [];
    const kept = files.filter((item) => item.path !== path && !item.path.startsWith(`${path}/`));
    bases.set(key, kept);
    res.json({ deleted: kept.length < files.length });
  });

  app.post('/api/v1/agent/knowledge/move', (req, res) => {
    const key = keyOf(req.body?.scope, req.body?.team_id);
    const files = bases.get(key) ?? [];
    const found = files.find((item) => item.path === req.body?.from);
    if (!found) {
      res.status(404).json({ error: 'not_found', message: 'knowledge file not found' });
      return;
    }
    found.path = String(req.body?.to ?? found.path);
    found.updated_at_micros = now;
    res.json({ moved: true });
  });

  // Only the file names and the scope matter here; every file is taken as a short text.
  app.post(
    '/api/v1/agent/knowledge/upload',
    express.raw({ type: 'multipart/form-data', limit: '8mb' }),
    (req, res) => {
      const text = Buffer.isBuffer(req.body) ? req.body.toString('utf8') : '';
      const field = (name: string) => new RegExp(`name="${name}"\\r\\n\\r\\n([^\\r]*)`).exec(text)?.[1];
      const key = keyOf(field('scope'), field('team_id'));
      const names = [...text.matchAll(/name="file"; filename="([^"]+)"/g)].map((match) => match[1] ?? '');
      const files = bases.get(key) ?? [];
      const written = names.filter((name) => !name.split('/').some((part) => part.startsWith('.')));
      const skipped = names
        .filter((name) => !written.includes(name))
        .map((path) => ({ path, reason: 'hidden_or_invalid_path' }));
      bases.set(key, [
        ...files.filter((item) => !written.includes(item.path)),
        ...written.map((path) => file(path, `# ${path}\n`, 0)),
      ]);
      res.json({ written: written.length, skipped });
    },
  );

  // ── Memory ──
  const memory = (
    id: string,
    kind: Memory['kind'],
    name: string,
    description: string,
    body: string,
    ago: number,
    source: { id: string; title: string } | null = null,
  ): Memory => ({
    id,
    scope: 'personal',
    team_id: null,
    kind,
    name,
    description,
    body,
    source_chat_id: source?.id ?? null,
    source_chat_title: source?.title ?? null,
    created_by: 'dev',
    updated_by: 'dev',
    created_at_micros: now - ago,
    updated_at_micros: now - ago,
    forgotten_at_micros: null,
  });
  const memories: Memory[] = [
    memory('memory-time-zone', 'preference', 'Time zone', 'When writing times in answers', 'Write times in UTC+8.', 20 * MINUTE),
    memory(
      'memory-payments-db',
      'fact',
      'Payments database',
      'When payments look slow',
      'Payments runs on the primary Postgres in eu-west-1.',
      2 * 60 * MINUTE,
      { id: 'chat-payments-latency', title: 'Payments latency in eu-west-1' },
    ),
  ];
  let distill = false;

  app.get('/api/v1/agent/memories', (req, res) => {
    const team = req.query.scope === 'team' ? String(req.query.team_id ?? '') : null;
    res.json({
      memories: memories.filter(
        (item) => !item.forgotten_at_micros && (team ? item.team_id === team : item.scope === 'personal'),
      ),
    });
  });
  app.patch('/api/v1/agent/memories/:id', (req, res) => {
    const found = memories.find((item) => item.id === req.params.id);
    if (!found) {
      res.status(404).json({ error: 'not_found', message: 'memory not found' });
      return;
    }
    Object.assign(found, {
      kind: req.body?.kind ?? found.kind,
      name: req.body?.name ?? found.name,
      description: req.body?.description ?? found.description,
      body: req.body?.body ?? found.body,
      updated_at_micros: now,
    });
    res.json(found);
  });
  app.delete('/api/v1/agent/memories/:id', (req, res) => {
    const index = memories.findIndex((item) => item.id === req.params.id);
    if (index >= 0) memories.splice(index, 1);
    res.json({ deleted: index >= 0 });
  });
  app.post('/api/v1/agent/memories/:id/restore', (req, res) => {
    const found = memories.find((item) => item.id === req.params.id);
    if (!found) {
      res.status(404).json({ error: 'not_found', message: 'memory not found' });
      return;
    }
    found.forgotten_at_micros = null;
    res.json(found);
  });
  app.get('/api/v1/agent/memory-settings', (_req, res) =>
    res.json({ distill_after_chat: distill, can_edit: true, updated_at_micros: null }),
  );
  app.put('/api/v1/agent/memory-settings', (req, res) => {
    distill = Boolean(req.body?.distill_after_chat);
    res.json({ distill_after_chat: distill, can_edit: true, updated_at_micros: now });
  });

  // ── @ references ──
  const candidates = [
    { kind: 'incident', id: 'inc-2291', label: 'Public API answers 16% of requests with 502', detail: 'Critical · Acknowledged' },
    { kind: 'incident', id: 'inc-2292', label: 'Checkout p95 at 2.4 s, three times its usual', detail: 'Error · Open' },
    { kind: 'alert_rule', id: 'rule-api-5xx', label: 'API 5xx rate', detail: 'enabled' },
    { kind: 'service', id: 'checkout', label: 'checkout', detail: '' },
    { kind: 'dashboard', id: 'dash-service-health', label: 'Service health', detail: 'sre' },
    { kind: 'stream', id: 'stream-logs-default', label: 'logs/default', detail: 'logs' },
  ];
  app.get('/api/v1/agent/references', (req, res) => {
    const q = String(req.query.q ?? '').toLowerCase();
    res.json({
      results: candidates.filter(
        (item) =>
          (!req.query.kind || item.kind === req.query.kind) &&
          (!q || item.label.toLowerCase().includes(q) || item.id.toLowerCase().includes(q)),
      ),
    });
  });

  // ── Investigations started from an incident ──
  app.get('/api/v1/agent/investigate/incident/:id', (req, res) => {
    const found = candidates.find((item) => item.kind === 'incident' && item.id === req.params.id);
    res.json({
      incident: {
        id: req.params.id,
        summary: found?.label ?? 'Incident',
        severity: 'critical',
        status: 'acknowledged',
        created_at_micros: now - 38 * MINUTE,
      },
      team: { id: 'team-platform', name: 'Platform' },
    });
  });
  app.post('/api/v1/agent/chat/:id/rca', (req, res) =>
    res.json({ incident_id: 'inc-2291', updated_at_micros: now, message_id: req.body?.message_id }),
  );
}

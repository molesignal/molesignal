/**
 * Mock Mole Agent resources that belong to a scope: skills (built-in, an
 * instance folder, uploaded, from a repository), skill repositories, the
 * organization's network policy for members, and what members do with MCP
 * servers (turn one on or off, its tools, their own credential, importing a
 * client configuration). The signed-in `dev` user holds `agent.manage`.
 */
import { Buffer } from 'node:buffer';

import express, { type Express } from 'express';

import type { McpServer } from '../../src/api/agent/mcpServers';
import type { Skill, SkillDetail, SkillRepository } from '../../src/api/agent/skills';

function skill(
  id: string,
  name: string,
  description: string,
  extra: Partial<Skill> = {},
): Skill {
  return {
    id,
    name,
    title: name,
    description,
    version: null,
    source: 'upload',
    scope: 'org',
    team_id: null,
    owner_id: null,
    enabled: true,
    editable: true,
    moderatable: true,
    hash: `${id}0123456789abcdef`,
    file_count: 1,
    argument_hint: null,
    allowed_tools: [],
    tags: [],
    examples: [],
    requires_runner: false,
    repository_id: null,
    package_name: `${name}.zip`,
    dir: null,
    updated_at_micros: null,
    ...extra,
  };
}

/** A zip with nothing in it: what the package route answers. */
const EMPTY_ZIP = Buffer.from([0x50, 0x4b, 0x05, 0x06, ...new Array(18).fill(0)]);

export function registerAgentResourceRoutes(
  app: Express,
  nowIso: string,
  mcpServers: Array<Record<string, unknown>>,
): void {
  const now = Date.parse(nowIso) * 1000;

  const skills: Skill[] = [
    skill('builtin:incident-triage', 'incident-triage', 'Confirm what is broken and who owns it', {
      title: 'Incident triage',
      source: 'builtin',
      scope: 'instance',
      editable: false,
      examples: ['What is broken in checkout right now?'],
      package_name: null,
    }),
    skill('path:k8s-debug', 'k8s-debug', 'Look into a failing Kubernetes workload', {
      source: 'path',
      scope: 'instance',
      editable: false,
      requires_runner: true,
      dir: '/etc/molesignal/skills/k8s-debug',
      package_name: null,
    }),
    skill('skill-postmortem', 'postmortem', 'Write a postmortem from an incident', {
      version: '1.2.0',
      allowed_tools: ['get_incident', 'query_logs'],
      updated_at_micros: now - 86_400_000_000,
    }),
    skill('skill-runbook', 'runbook', 'Follow the platform team runbook', {
      source: 'repository',
      scope: 'team',
      team_id: 'team-platform',
      repository_id: 'repo-platform',
      dir: 'skills/runbook',
      package_name: null,
    }),
  ];

  const repositories: Array<SkillRepository> = [
    {
      id: 'repo-platform',
      org_id: 'acme-prod',
      scope: { scope: 'team', team_id: 'team-platform' },
      url: 'https://github.com/acme/platform-skills',
      host: 'github',
      archive_url: null,
      git_ref: 'main',
      dir: 'skills',
      auto_sync: true,
      token_set: false,
      token_last4: null,
      status: 'partial',
      last_error: 'skills/broken: SKILL.md needs a `description`',
      last_commit: '9f2c1e4b7a00',
      last_synced_at: now - 3_600_000_000,
      created_by: 'dev',
      updated_by: 'dev',
      editable: true,
    },
  ];

  let networkPolicy = {
    private_hosts: ['git.corp.example'],
    private_cidrs: ['10.20.0.0/16'],
    updated_by: 'dev',
    updated_at: now - 7 * 86_400_000_000,
  };

  const detail = (item: Skill): SkillDetail => ({
    ...item,
    body: `# ${item.name}\n\n${item.description}.`,
    files: [
      { path: 'SKILL.md', size: 412, text: true },
      ...(item.name === 'postmortem' ? [{ path: 'templates/report.md', size: 96, text: true }] : []),
    ],
  });

  app.get('/api/v1/agent/skills', (_req, res) =>
    res.json({
      skills,
      paths: {
        roots: ['/etc/molesignal/skills'],
        problems: [{ dir: '/etc/molesignal/skills/broken', message: 'SKILL.md needs a `name`' }],
        scanned_at_micros: now - 60_000_000,
      },
    }),
  );

  // The package is read as raw bytes: only its file name and the `replace` field matter here.
  app.post(
    '/api/v1/agent/skills',
    express.raw({ type: 'multipart/form-data', limit: '30mb' }),
    (req, res) => {
      const text = Buffer.isBuffer(req.body) ? req.body.toString('latin1') : '';
      const fileName = /filename="([^"]+)"/.exec(text)?.[1] ?? 'skill.zip';
      const name = fileName.replace(/\.(zip|tar\.gz|tgz)$/i, '').toLowerCase();
      const replace = /name="replace"\r\n\r\ntrue/.test(text);
      const existing = skills.find((item) => item.name === name && item.scope !== 'instance');
      if (existing && !replace) {
        return res.status(409).json({
          error: 'skill_exists',
          code: 'skill_exists',
          name,
          message: `a skill named \`${name}\` already exists in this scope`,
        });
      }
      if (existing) {
        existing.version = '2.0.0';
        existing.updated_at_micros = now;
      } else {
        skills.push(skill(`skill-${name}`, name, `Uploaded from ${fileName}`, { scope: 'personal', owner_id: 'dev' }));
      }
      return res.json({ ok: true });
    },
  );

  app.post('/api/v1/agent/skills/rescan', (_req, res) =>
    res.json({ roots: ['/etc/molesignal/skills'], problems: [], scanned_at_micros: now }),
  );

  app.get('/api/v1/agent/skills/:id', (req, res) => {
    const found = skills.find((item) => item.id === req.params.id);
    return found ? res.json(detail(found)) : res.status(404).json({ error: 'not found' });
  });
  app.get('/api/v1/agent/skills/:id/file', (req, res) =>
    res.json({ path: req.query.path, content: `Contents of ${String(req.query.path)}` }),
  );
  app.get('/api/v1/agent/skills/:id/package', (req, res) => {
    res.setHeader('content-type', 'application/octet-stream');
    res.setHeader('content-disposition', `attachment; filename="${req.params.id}.zip"`);
    res.send(EMPTY_ZIP);
  });
  app.patch('/api/v1/agent/skills/:id', (req, res) => {
    const found = skills.find((item) => item.id === req.params.id);
    if (!found) return res.status(404).json({ error: 'not found' });
    if (typeof req.body?.enabled === 'boolean') found.enabled = req.body.enabled;
    if (typeof req.body?.scope === 'string') {
      found.scope = req.body.scope;
      found.team_id = req.body.scope === 'team' ? (req.body.team_id ?? null) : null;
      found.owner_id = req.body.scope === 'personal' ? 'dev' : null;
    }
    return res.json(detail(found));
  });
  app.delete('/api/v1/agent/skills/:id', (req, res) => {
    const index = skills.findIndex((item) => item.id === req.params.id);
    if (index >= 0) skills.splice(index, 1);
    res.json({ deleted: true });
  });

  const report = { status: 'ok', imported: 2, updated: 0, unchanged: 1, removed: 0, problems: [], commit: 'a1b2c3d4e5f6' };

  app.get('/api/v1/agent/skill-repositories', (_req, res) => res.json({ repositories }));
  app.post('/api/v1/agent/skill-repositories', (req, res) => {
    const repository: SkillRepository = {
      id: `repo-${repositories.length + 1}`,
      org_id: 'acme-prod',
      scope:
        req.body?.scope === 'team'
          ? { scope: 'team', team_id: String(req.body.team_id) }
          : req.body?.scope === 'org'
            ? { scope: 'org' }
            : { scope: 'personal', owner_id: 'dev' },
      url: String(req.body?.url ?? ''),
      host: req.body?.host && req.body.host !== 'auto' ? req.body.host : 'github',
      archive_url: req.body?.archive_url ?? null,
      git_ref: String(req.body?.git_ref ?? ''),
      dir: String(req.body?.dir ?? ''),
      auto_sync: Boolean(req.body?.auto_sync),
      token_set: Boolean(req.body?.token),
      token_last4: req.body?.token ? String(req.body.token).slice(-4) : null,
      status: 'ok',
      last_error: null,
      last_commit: report.commit,
      last_synced_at: now,
      created_by: 'dev',
      updated_by: 'dev',
      editable: true,
    };
    repositories.push(repository);
    res.json({ repository, sync: report });
  });
  app.put('/api/v1/agent/skill-repositories/:id', (req, res) => {
    const found = repositories.find((item) => item.id === req.params.id);
    if (!found) return res.status(404).json({ error: 'not found' });
    Object.assign(found, {
      url: req.body?.url ?? found.url,
      git_ref: req.body?.git_ref ?? found.git_ref,
      dir: req.body?.dir ?? found.dir,
      auto_sync: Boolean(req.body?.auto_sync),
    });
    return res.json({ repository: found, sync: report });
  });
  app.delete('/api/v1/agent/skill-repositories/:id', (req, res) => {
    const index = repositories.findIndex((item) => item.id === req.params.id);
    if (index >= 0) repositories.splice(index, 1);
    res.json({ deleted: true });
  });
  app.post('/api/v1/agent/skill-repositories/:id/sync', (_req, res) =>
    res.json({ sync: { ...report, imported: 0, updated: 1, unchanged: 2 } }),
  );

  app.get('/api/v1/agent/network-policy', (_req, res) => res.json(networkPolicy));
  app.put('/api/v1/agent/network-policy', (req, res) => {
    networkPolicy = {
      private_hosts: Array.isArray(req.body?.private_hosts) ? req.body.private_hosts : [],
      private_cidrs: Array.isArray(req.body?.private_cidrs) ? req.body.private_cidrs : [],
      updated_by: 'dev',
      updated_at: now,
    };
    res.json(networkPolicy);
  });

  // What members do with MCP servers; the servers themselves are the backend mock's.
  const server = (id: string) => mcpServers.find((item) => item.id === id) as McpServer | undefined;

  app.post('/api/v1/agent/mcp-servers/import', (req, res) => {
    const text = typeof req.body?.config === 'string' ? req.body.config : JSON.stringify(req.body?.config ?? {});
    let parsed: { mcpServers?: Record<string, Record<string, unknown>> };
    try {
      parsed = JSON.parse(text) as typeof parsed;
    } catch {
      return res.status(400).json({ error: 'the configuration is not valid JSON', code: 'invalid' });
    }
    const created = Object.entries(parsed.mcpServers ?? {}).map(([name, config], index) => {
      const headers = Object.entries((config.headers as Record<string, string> | undefined) ?? {});
      const missing = headers.filter(([, value]) => value.includes('${')).map(([key]) => key);
      const made = {
        id: `mcp-import-${index + 1}`,
        org_id: 'acme-prod',
        scope: { scope: 'personal', owner_id: 'dev' },
        handle: name.toLowerCase(),
        name,
        transport: config.command ? 'stdio' : 'streamable_http',
        endpoint_url: (config.url as string | undefined) ?? null,
        command_template: config.command ? `${String(config.command)} ${((config.args as string[]) ?? []).join(' ')}` : null,
        auth_type: 'none',
        auth_header: null,
        credential_last4: null,
        credential_set: false,
        member_credential: null,
        headers: headers.map(([key]) => ({ name: key.toLowerCase(), secret: true, set: !missing.includes(key) })),
        env: [],
        private_only: false,
        allowed_domains: [],
        allowed_cidrs: [],
        follow_redirects: false,
        tls_verify: true,
        timeout_ms: 10_000,
        max_response_bytes: 1_048_576,
        enabled: true,
        status: 'unavailable',
        last_error: null,
        created_by: 'dev',
        created_at: now,
        updated_at: now,
        tool_count: 0,
        prompt_count: 0,
        editable: true,
        moderatable: true,
        my_credential: null,
      };
      mcpServers.push(made);
      return { ...made, missing };
    });
    return res.json({ created, skipped: [] });
  });

  app.patch('/api/v1/agent/mcp-servers/:id', (req, res) => {
    const found = server(req.params.id);
    if (!found) return res.status(404).json({ error: 'not found' });
    found.enabled = Boolean(req.body?.enabled);
    return res.json(found);
  });
  app.post('/api/v1/agent/mcp-servers/:id/enable-read-only', (req, res) => {
    const found = server(req.params.id);
    if (!found) return res.status(404).json({ error: 'not found' });
    return res.json({ enabled: 1, tools: [] });
  });
  app.patch('/api/v1/agent/mcp-servers/:id/tools/:toolId', (req, res) =>
    res.json({ id: req.params.toolId, enabled: Boolean(req.body?.enabled) }),
  );
  app.put('/api/v1/agent/mcp-servers/:id/my-credential', (req, res) => {
    const found = server(req.params.id);
    if (!found) return res.status(404).json({ error: 'not found' });
    const credential = {
      server_id: found.id,
      user_id: 'dev',
      last4: String(req.body?.value ?? '').slice(-4),
      updated_at: now,
    };
    found.my_credential = credential;
    return res.json({ credential });
  });
  app.delete('/api/v1/agent/mcp-servers/:id/my-credential', (req, res) => {
    const found = server(req.params.id);
    if (!found) return res.status(404).json({ error: 'not found' });
    found.my_credential = null;
    return res.json({ deleted: true });
  });
}

import type { Express, Request, Response } from 'express';

import type {
  EnrollmentToken,
  NodeAgent,
  NodeTask,
  NodeTaskQuota,
  NodeTaskState,
} from '../../src/api/nodeAgents';

const MINUTE_MICROS = 60_000_000;

const CA_PEM = '-----BEGIN CERTIFICATE-----\nMIIBjDCCATKgAwIBAgIUMockNodeAgentCA\n-----END CERTIFICATE-----\n';

function node(nowMicros: number, overrides: Partial<NodeAgent>): NodeAgent {
  return {
    id: 'node-web-1',
    orgId: 'org-default',
    hostId: 'a1b2c3d4e5',
    bootId: 'boot-1',
    hostname: 'web-1.prod',
    labels: { rack: 'r1', env: 'prod' },
    agentVersion: '0.4.0',
    protocolVersion: '1.0',
    capabilities: { taskKinds: ['autotracing_run', 'profile', 'tool'], features: [], maxRunningTasks: 2 },
    certificateSerial: '29:a5:17:0e:d6:bc:f8:32',
    certificateExpiresAt: nowMicros + 20 * 24 * 60 * MINUTE_MICROS,
    enrollmentTokenId: 'tok-prod',
    health: { state: 'healthy', messages: [] },
    collectorStates: [
      { name: 'cpu_util', state: 'running', message: '' },
      { name: 'softirq', state: 'running', message: '' },
    ],
    appliedConfigRevision: 2,
    desiredConfig: {
      revision: 2,
      config: { collectors: { deny: ['ras'] }, profiling: { oncpu: true } },
      updatedBy: 'user-1',
      updatedAt: nowMicros - 90 * MINUTE_MICROS,
    },
    configStatus: { revision: 2, state: 'applied', reason: null, reportedAt: nowMicros - 88 * MINUTE_MICROS },
    lastHeartbeatAt: nowMicros - 20_000_000,
    lastConnectedAt: nowMicros - 3 * 60 * MINUTE_MICROS,
    drainRequestedAt: null,
    drainDeadline: null,
    drainedAt: null,
    revokedAt: null,
    revokeReason: null,
    enrolledAt: nowMicros - 7 * 24 * 60 * MINUTE_MICROS,
    createdAt: nowMicros - 7 * 24 * 60 * MINUTE_MICROS,
    updatedAt: nowMicros - 20_000_000,
    status: 'online',
    ...overrides,
  };
}

function seedNodes(nowMicros: number): NodeAgent[] {
  return [
    node(nowMicros, {}),
    node(nowMicros, {
      id: 'node-db-1',
      hostId: 'f6e5d4c3b2',
      hostname: 'db-1.prod',
      labels: { rack: 'r2', env: 'prod' },
      lastHeartbeatAt: nowMicros - 25 * MINUTE_MICROS,
      health: { state: 'degraded', messages: ['bpf map full'] },
      status: 'offline',
    }),
    node(nowMicros, {
      id: 'node-batch-1',
      hostId: '0a9b8c7d6e',
      hostname: 'batch-1.prod',
      agentVersion: '0.3.9',
      labels: { env: 'prod' },
      capabilities: { taskKinds: ['profile'], features: [], maxRunningTasks: 1 },
      drainRequestedAt: nowMicros - 5 * MINUTE_MICROS,
      drainDeadline: nowMicros + 5 * MINUTE_MICROS,
      status: 'draining',
    }),
  ];
}

function seedTasks(nowMicros: number): NodeTask[] {
  const base = {
    orgId: 'org-default',
    attempt: 1,
    maxAttempts: 3,
    cancelRequestedAt: null,
    cancelRequestedBy: null,
    createdBy: 'user-1',
    failure: null,
  };

  return [
    {
      ...base,
      id: '0b1f8a52-7f21-4f0e-9a51-2a8c3c1d6e01',
      nodeId: 'node-web-1',
      kind: 'tool',
      spec: { tool: 'tcpshark', mode: 'retransmit', duration_secs: 60 },
      state: 'running',
      availableAt: nowMicros - 2 * MINUTE_MICROS,
      leaseExpiresAt: nowMicros + 2 * MINUTE_MICROS,
      deadline: nowMicros + 58 * MINUTE_MICROS,
      result: null,
      createdAt: nowMicros - 2 * MINUTE_MICROS,
      updatedAt: nowMicros - MINUTE_MICROS,
      finishedAt: null,
    },
    {
      ...base,
      id: '6c0d7e14-1b2a-4c3d-8e9f-0a1b2c3d4e02',
      nodeId: 'node-web-1',
      kind: 'tool',
      spec: { tool: 'perf', duration_secs: 10, frequency_hz: 99 },
      state: 'succeeded',
      availableAt: nowMicros - 40 * MINUTE_MICROS,
      leaseExpiresAt: null,
      deadline: nowMicros + 20 * MINUTE_MICROS,
      result: {
        resultId: '6c0d7e14-1b2a-4c3d-8e9f-0a1b2c3d4e02/1',
        attempt: 1,
        status: 'succeeded',
        summary: 'events=1 lost_samples=0',
        artifactRefs: ['pprof:sha256:9f2c'],
        receivedAt: nowMicros - 38 * MINUTE_MICROS,
      },
      createdAt: nowMicros - 40 * MINUTE_MICROS,
      updatedAt: nowMicros - 38 * MINUTE_MICROS,
      finishedAt: nowMicros - 38 * MINUTE_MICROS,
    },
  ];
}

function seedTokens(nowMicros: number): EnrollmentToken[] {
  return [
    {
      id: 'tok-prod',
      orgId: 'org-default',
      description: 'Production hosts',
      labels: { env: 'prod' },
      maxUses: 20,
      uses: 3,
      expiresAt: nowMicros + 6 * 24 * 60 * MINUTE_MICROS,
      revokedAt: null,
      lastUsedAt: nowMicros - 7 * 24 * 60 * MINUTE_MICROS,
      createdBy: 'user-1',
      createdAt: nowMicros - 8 * 24 * 60 * MINUTE_MICROS,
    },
  ];
}

/** `/node-agents/*`: the inventory, tasks, tokens and quota, kept in memory for one server. */
export function registerNodeAgentRoutes(app: Express, nowIso: string): void {
  const nowMicros = Date.parse(nowIso) * 1000;
  const nodes = seedNodes(nowMicros);
  const tasks = seedTasks(nowMicros);
  const tokens = seedTokens(nowMicros);
  let quota: NodeTaskQuota = {
    maxActivePerOrg: 50,
    maxActivePerNode: 4,
    maxCreatedPerHourPerOrg: 600,
    maxCreatedPerHourPerNode: 60,
  };
  let sequence = 1;

  const nodeOr404 = (req: Request, res: Response) => {
    const found = nodes.find((candidate) => candidate.id === req.params.nodeId);
    if (!found) res.status(404).json({ code: 'not_found', message: 'node not found' });
    return found;
  };

  app.get('/api/v1/node-agents/nodes', (_req, res) => res.json(nodes));

  app.post('/api/v1/node-agents/nodes/:nodeId/drain', (req, res) => {
    const found = nodeOr404(req, res);
    if (!found) return;
    const seconds = Number(req.body?.deadlineSeconds ?? 600);
    found.drainRequestedAt = nowMicros;
    found.drainDeadline = nowMicros + seconds * 1_000_000;
    found.status = 'draining';
    res.json(found);
  });

  app.post('/api/v1/node-agents/nodes/:nodeId/resume', (req, res) => {
    const found = nodeOr404(req, res);
    if (!found) return;
    found.drainRequestedAt = null;
    found.drainDeadline = null;
    found.drainedAt = null;
    found.status = 'online';
    res.json(found);
  });

  app.post('/api/v1/node-agents/nodes/:nodeId/revoke', (req, res) => {
    const found = nodeOr404(req, res);
    if (!found) return;
    found.revokedAt = nowMicros;
    found.revokeReason = String(req.body?.reason ?? '');
    found.status = 'revoked';

    for (const task of tasks) {
      if (task.nodeId === found.id && ['queued', 'offered', 'running'].includes(task.state)) {
        task.state = 'cancelled';
        task.failure = 'the node was revoked';
        task.finishedAt = nowMicros;
        task.leaseExpiresAt = null;
      }
    }

    res.json(found);
  });

  app.put('/api/v1/node-agents/nodes/:nodeId/config', (req, res) => {
    const found = nodeOr404(req, res);
    if (!found) return;
    found.desiredConfig = {
      revision: (found.desiredConfig?.revision ?? 0) + 1,
      config: req.body?.config ?? {},
      updatedBy: 'user-1',
      updatedAt: nowMicros,
    };
    res.json(found);
  });

  app.get('/api/v1/node-agents/enrollment-tokens', (_req, res) => res.json(tokens));

  app.post('/api/v1/node-agents/enrollment-tokens', (req, res) => {
    const token: EnrollmentToken = {
      id: `tok-new-${sequence++}`,
      orgId: 'org-default',
      description: String(req.body?.description ?? ''),
      labels: req.body?.labels ?? {},
      maxUses: Number(req.body?.maxUses ?? 1),
      uses: 0,
      expiresAt: nowMicros + Number(req.body?.ttlMinutes ?? 60) * MINUTE_MICROS,
      revokedAt: null,
      lastUsedAt: null,
      createdBy: 'user-1',
      createdAt: nowMicros,
    };
    tokens.unshift(token);
    res.status(201).json({
      ...token,
      plaintext: 'msnode_3f9c0d1e2b4a5c6d7e8f90a1b2c3d4e5f60718293a4b5c6d7e8f9a0b1c2d3e',
      endpoint: 'https://localhost:5086',
      caCertificatePem: CA_PEM,
    });
  });

  app.post('/api/v1/node-agents/enrollment-tokens/:tokenId/revoke', (req, res) => {
    const token = tokens.find((candidate) => candidate.id === req.params.tokenId);
    if (!token) return void res.status(404).json({ code: 'not_found', message: 'token not found' });
    token.revokedAt = nowMicros;
    res.json(token);
  });

  app.get('/api/v1/node-agents/tasks', (req, res) => {
    const nodeId = typeof req.query.nodeId === 'string' ? req.query.nodeId : undefined;
    const states = typeof req.query.state === 'string' ? req.query.state.split(',') : [];

    res.json(
      tasks.filter(
        (task) => (!nodeId || task.nodeId === nodeId) && (states.length === 0 || states.includes(task.state)),
      ),
    );
  });

  app.post('/api/v1/node-agents/tasks', (req, res) => {
    const timeout = Number(req.body?.timeoutSeconds ?? 3600) * 1_000_000;
    const task: NodeTask = {
      id: `9e8d7c6b-5a49-4382-9170-${String(sequence++).padStart(12, '0')}`,
      orgId: 'org-default',
      nodeId: String(req.body?.nodeId ?? ''),
      kind: req.body?.kind ?? 'tool',
      spec: req.body?.spec ?? {},
      state: 'queued' satisfies NodeTaskState,
      attempt: 1,
      maxAttempts: Number(req.body?.maxAttempts ?? 3),
      availableAt: nowMicros,
      leaseExpiresAt: null,
      deadline: nowMicros + timeout,
      cancelRequestedAt: null,
      cancelRequestedBy: null,
      result: null,
      failure: null,
      createdBy: 'user-1',
      createdAt: nowMicros,
      updatedAt: nowMicros,
      finishedAt: null,
    };
    tasks.unshift(task);
    res.status(201).json(task);
  });

  app.post('/api/v1/node-agents/tasks/:taskId/cancel', (req, res) => {
    const task = tasks.find((candidate) => candidate.id === req.params.taskId);
    if (!task) return void res.status(404).json({ code: 'not_found', message: 'task not found' });

    if (task.state === 'queued') {
      task.state = 'cancelled';
      task.finishedAt = nowMicros;
    } else if (task.state === 'offered' || task.state === 'running') {
      task.cancelRequestedAt = nowMicros;
      task.cancelRequestedBy = 'user-1';
    }

    res.json(task);
  });

  app.get('/api/v1/node-agents/task-quota', (_req, res) => res.json(quota));

  app.put('/api/v1/node-agents/task-quota', (req, res) => {
    quota = { ...quota, ...req.body };
    res.json(quota);
  });
}

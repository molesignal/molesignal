/**
 * Mock data gaps and drills of the Quality tab, the way `/api/v1/agent/quality/gaps` and
 * `/api/v1/agent/quality/drills…` answer: gaps of three kinds found in scored answers (whose IDs
 * are those of `mockQuality.ts`, so an example opens in the answer drawer), two drills — one graded,
 * one never run — and two candidates, one already a drill. A started run is graded once the runs
 * are read twice. Kept per test.
 */
import type { Express } from 'express';

import type { Drill, DrillCandidate, DrillRun, DrillSummary } from '../../src/api/agent/drills';
import type { DataGaps } from '../../src/api/agent/quality';

const MINUTE = 60_000_000;
const DAY = 24 * 60 * MINUTE;

function gapsOf(now: number): DataGaps {
  return {
    answers: 24,
    withGaps: 6,
    kinds: [
      { kind: 'data_map', answers: 4 },
      { kind: 'field_alignment', answers: 2 },
      { kind: 'feature_tools', answers: 1 },
    ],
    groups: [
      {
        kind: 'data_map',
        rule: 'repeated_queries',
        stream: 'nginx',
        answers: 3,
        calls: 11,
        misses: 8,
        lastAt: now - 2 * 60 * MINUTE,
        examples: [
          {
            targetId: 'message-disk-guess',
            chatId: 'chat-disk',
            source: 'team_chat',
            question: 'Is the disk of db-2 filling up?',
            targetAt: now - 2 * 60 * MINUTE,
          },
        ],
      },
      {
        kind: 'field_alignment',
        rule: 'service_mismatch',
        stream: 'app_logs',
        service: 'order-api',
        answers: 2,
        calls: 2,
        misses: 2,
        lastAt: now - 5 * 60 * MINUTE,
        examples: [
          {
            targetId: 'message-checkout-latency',
            chatId: 'chat-checkout',
            source: 'team_chat',
            question: 'Why did checkout latency rise after 08:10?',
            targetAt: now - 5 * 60 * MINUTE,
          },
        ],
      },
      {
        kind: 'feature_tools',
        rule: 'raw_log_pulls',
        stream: 'nginx',
        answers: 1,
        calls: 3,
        misses: 0,
        lastAt: now - DAY,
        examples: [],
      },
    ],
  };
}

function seedDrills(now: number): DrillSummary[] {
  const graded: DrillRun = {
    id: 'drill-run-pool-1',
    orgId: 'org-1',
    drillId: 'drill-pool',
    chatId: 'chat-drill-pool-1',
    runId: 'agent-run-drill-1',
    status: 'done',
    score: 0.75,
    reasoning: 'Names the exhausted payment pool, but ranks the deploy first.',
    reason: null,
    detail: null,
    model: 'gpt-4.1',
    promptVersion: 2,
    graderModel: 'gpt-4.1-mini',
    language: 'en-us',
    batchId: null,
    attempts: 1,
    startedBy: 'dev',
    createdAt: now - 3 * 60 * MINUTE,
    finishedAt: now - 3 * 60 * MINUTE + 4 * MINUTE,
    updatedAt: now - 3 * 60 * MINUTE + 4 * MINUTE,
  };

  return [
    {
      id: 'drill-pool',
      orgId: 'org-1',
      incidentId: 'incident-checkout-pool',
      title: 'Checkout 5xx above 5%',
      reference: 'The payment connection pool was too small for the Monday traffic and ran out at 08:10.',
      source: 'postmortem',
      createdBy: 'dev',
      createdAt: now - 2 * DAY,
      updatedAt: now - 2 * DAY,
      runs: 1,
      meanScore: 0.75,
      lastRun: graded,
    },
    {
      id: 'drill-dns',
      orgId: 'org-1',
      incidentId: 'incident-dns',
      title: 'Search timeouts in eu-west',
      reference: 'A DNS resolver of eu-west dropped queries after its config was rolled out.',
      source: 'manual',
      createdBy: 'dev',
      createdAt: now - DAY,
      updatedAt: now - DAY,
      runs: 0,
      meanScore: null,
      lastRun: null,
    },
  ];
}

function seedCandidates(now: number): DrillCandidate[] {
  return [
    {
      incidentId: 'incident-queue',
      summary: 'Order queue backlog over 10k',
      severity: 'error',
      createdAt: now - 6 * DAY,
      resolvedAt: now - 6 * DAY + 50 * MINUTE,
      rootCause: 'A consumer deploy left the order workers with one thread each.',
      drilled: false,
    },
    {
      incidentId: 'incident-checkout-pool',
      summary: 'Checkout 5xx above 5%',
      severity: 'critical',
      createdAt: now - 9 * DAY,
      resolvedAt: now - 9 * DAY + 30 * MINUTE,
      rootCause: 'The payment connection pool was too small.',
      drilled: true,
    },
  ];
}

export function registerDrillRoutes(app: Express, now: number) {
  const drills = seedDrills(now);
  const candidates = seedCandidates(now);
  const seeded = drills[0]?.lastRun ?? null;
  const runs = new Map<string, DrillRun[]>([['drill-pool', seeded ? [seeded] : []]]);
  let reads = 0;

  const summaryOf = (drill: DrillSummary): DrillSummary => {
    const list = runs.get(drill.id) ?? [];
    const graded = list.filter((run) => run.status === 'done' && run.score !== null);
    return {
      ...drill,
      runs: list.length,
      meanScore: graded.length ? graded.reduce((sum, run) => sum + (run.score ?? 0), 0) / graded.length : null,
      lastRun: list[0] ?? null,
    };
  };

  const start = (drill: DrillSummary, batchId: string | null): DrillRun => {
    const run: DrillRun = {
      id: `drill-run-${drill.id}-${(runs.get(drill.id)?.length ?? 0) + 1}`,
      orgId: 'org-1',
      drillId: drill.id,
      chatId: `chat-${drill.id}-${Date.now()}`,
      runId: `agent-run-${drill.id}`,
      status: 'running',
      score: null,
      reasoning: '',
      reason: null,
      detail: null,
      model: null,
      promptVersion: null,
      graderModel: null,
      language: 'en-us',
      batchId,
      attempts: 0,
      startedBy: 'dev',
      createdAt: now,
      finishedAt: null,
      updatedAt: now,
    };
    runs.set(drill.id, [run, ...(runs.get(drill.id) ?? [])]);
    reads = 0;
    return run;
  };

  /** A run under way is graded once the runs were read twice. */
  const settle = () => {
    reads += 1;
    if (reads < 2) return;
    for (const list of runs.values()) {
      for (const run of list) {
        if (run.status !== 'running') continue;
        Object.assign(run, {
          status: 'done',
          score: 1,
          reasoning: 'Finds the resolver that dropped queries after the rollout.',
          model: 'gpt-4.1',
          promptVersion: 2,
          graderModel: 'gpt-4.1-mini',
          attempts: 1,
          finishedAt: now + MINUTE,
          updatedAt: now + MINUTE,
        });
      }
    }
  };

  app.get('/api/v1/agent/quality/gaps', (req, res) =>
    res.json({ days: Number(req.query.days ?? 30), gaps: gapsOf(now) }),
  );

  app.get('/api/v1/agent/quality/drills', (_req, res) => {
    settle();
    res.json({ drills: drills.map(summaryOf) });
  });

  app.get('/api/v1/agent/quality/drills/candidates', (_req, res) => res.json({ candidates }));

  app.post('/api/v1/agent/quality/drills', (req, res) => {
    const candidate = candidates.find((item) => item.incidentId === req.body?.incidentId);
    const reference = typeof req.body?.reference === 'string' ? req.body.reference : null;
    if (!candidate && !reference) {
      return res.status(400).json({ error: 'invalid', message: 'write the reference root cause for the drill' });
    }
    const drill: Drill = {
      id: `drill-${drills.length + 1}`,
      orgId: 'org-1',
      incidentId: String(req.body?.incidentId),
      title: candidate?.summary ?? String(req.body?.incidentId),
      reference: reference ?? candidate?.rootCause ?? '',
      source: reference ? 'manual' : 'postmortem',
      createdBy: 'dev',
      createdAt: now,
      updatedAt: now,
    };
    drills.unshift({ ...drill, runs: 0, meanScore: null, lastRun: null });
    if (candidate) candidate.drilled = true;
    return res.json(drill);
  });

  app.post('/api/v1/agent/quality/drills/runs', (req, res) => {
    const named: string[] | undefined = req.body?.drillIds;
    const batchId = 'batch-1';
    const started = drills
      .filter((drill) => !named || named.includes(drill.id))
      .filter((drill) => {
        const last = runs.get(drill.id)?.[0];
        return !last || (last.status !== 'running' && last.status !== 'grading');
      })
      .map((drill) => start(drill, batchId));
    res.json({ batchId, runs: started, skipped: drills.length - started.length });
  });

  app.delete('/api/v1/agent/quality/drills/:id', (req, res) => {
    const index = drills.findIndex((drill) => drill.id === req.params.id);
    if (index < 0) return res.status(404).json({ error: 'not_found', message: 'not found' });
    drills.splice(index, 1);
    runs.delete(req.params.id);
    return res.json({ deleted: true });
  });

  app.get('/api/v1/agent/quality/drills/:id/runs', (req, res) => {
    const drill = drills.find((item) => item.id === req.params.id);
    if (!drill) return res.status(404).json({ error: 'not_found', message: 'not found' });
    settle();
    const plain: Drill = {
      id: drill.id,
      orgId: drill.orgId,
      incidentId: drill.incidentId,
      title: drill.title,
      reference: drill.reference,
      source: drill.source,
      createdBy: drill.createdBy,
      createdAt: drill.createdAt,
      updatedAt: drill.updatedAt,
    };
    return res.json({ drill: plain, runs: runs.get(drill.id) ?? [] });
  });

  app.post('/api/v1/agent/quality/drills/:id/runs', (req, res) => {
    const drill = drills.find((item) => item.id === req.params.id);
    if (!drill) return res.status(404).json({ error: 'not_found', message: 'not found' });
    return res.json({ run: start(drill, null) });
  });
}

/**
 * Mock Mole Agent quality: the overview of the last 30 days, four scored
 * answers (a team chat, an automation run, an incident RCA and a low-scored
 * team chat), their reviews, one evaluator, the scoring settings, and the check an
 * organization prompt goes through before it becomes the default — the way
 * `/api/v1/agent/quality` and `/api/v1/agent/settings/prompts/{id}/…` answer.
 * Kept per test.
 */
import type { Express } from 'express';

import type {
  Evaluator,
  PromptCheck,
  Review,
  ScoredAnswer,
  ScoringSettings,
} from '../../src/api/agent/quality';

const MINUTE = 60_000_000;
const DAY = 24 * 60 * MINUTE;

const EVALUATOR_ID = 'evaluator-accuracy';

function rules(score: number, dimensions: ScoredAnswer['scores'][number]['dimensions'], now: number) {
  return {
    id: `rules-${Math.round(score * 100)}-${now}`,
    targetKind: 'chat_answer' as const,
    targetId: '',
    scorer: 'rules',
    score,
    dimensions,
    reason: '',
    samples: 1,
    createdAt: now,
    updatedAt: now,
  };
}

function seedAnswers(now: number): ScoredAnswer[] {
  const evaluated = (targetId: string, score: number, reason: string, samples = 1) => ({
    id: `evaluator-${targetId}`,
    targetKind: 'chat_answer' as const,
    targetId: targetId,
    scorer: EVALUATOR_ID,
    score,
    dimensions: [],
    reason,
    samples,
    evaluatorModel: 'gpt-4.1-mini',
    createdAt: now,
    updatedAt: now,
  });

  return [
    {
      targetKind: 'chat_answer',
      targetId: 'message-checkout-latency',
      source: 'team_chat',
      question: 'Why did checkout latency rise after 08:10?',
      answer: 'checkout-api p95 rose from 180 ms to 420 ms when the payment pool ran out of connections.',
      attribution: { chatId: 'chat-checkout', teamId: 'team-platform', userId: 'dev', promptTemplateId: 'prompt-system-default', promptVersion: 1, model: 'gpt-4.1' },
      targetAt: now - 2 * 60 * MINUTE,
      scores: [
        {
          ...rules(1, [
            { dimension: 'tool_success', score: 1, detail: { failed: 0, total: 4 } },
            { dimension: 'evidence', score: 1, detail: { cited: 3, backed: true } },
            { dimension: 'query_validity', score: 1, detail: { invalid: 0, total: 2 } },
            { dimension: 'answer_format', score: 1, detail: { structured: true } },
            { dimension: 'completion', score: 1, detail: { status: 'succeeded' } },
          ], now),
          targetId: 'message-checkout-latency',
        },
        evaluated('message-checkout-latency', 0.75, 'Finds the exhausted pool from the metrics, but does not say when it started.'),
      ],
    },
    {
      targetKind: 'chat_answer',
      targetId: 'message-health-report',
      source: 'automation',
      question: 'Check the health of the checkout services over the last 24 hours.',
      answer: 'Healthy, except checkout-api: p95 latency rose after 08:10.',
      attribution: { chatId: 'chat-automation-health-1', automationId: 'automation-daily-health', userId: 'dev', model: 'gpt-4.1' },
      targetAt: now - 10 * 60 * MINUTE,
      scores: [
        {
          ...rules(0.83, [
            { dimension: 'tool_success', score: 0.75, detail: { failed: 1, total: 4 } },
            { dimension: 'answer_format', score: 1, detail: { structured: true } },
            { dimension: 'completion', score: 1, detail: { status: 'succeeded' } },
          ], now),
          targetId: 'message-health-report',
        },
      ],
    },
    {
      targetKind: 'incident_rca',
      targetId: 'incident-checkout-502',
      source: 'incident_rca',
      question: 'checkout-api 5xx above 5%',
      answer: 'The 502s start with the 14:02 deploy of checkout-api.',
      attribution: { promptKey: 'analysis.root_cause', model: 'gpt-4.1' },
      targetAt: now - 35 * MINUTE,
      scores: [
        {
          ...rules(1, [{ dimension: 'completion', score: 1, detail: { status: 'succeeded' } }], now),
          targetKind: 'incident_rca',
          targetId: 'incident-checkout-502',
        },
      ],
    },
    {
      targetKind: 'chat_answer',
      targetId: 'message-disk-guess',
      source: 'team_chat',
      question: 'Is the disk of db-2 filling up?',
      answer: 'Probably the WAL; it should be archived more often.',
      attribution: { chatId: 'chat-disk', teamId: 'team-platform', userId: 'dev', model: 'gpt-4.1' },
      targetAt: now - 26 * 60 * MINUTE,
      scores: [
        {
          ...rules(0.38, [
            { dimension: 'tool_success', score: 0.5, detail: { failed: 1, total: 2 } },
            { dimension: 'evidence', score: 0, detail: { cited: 0, backed: true } },
            { dimension: 'query_validity', score: 0.5, detail: { invalid: 1, total: 2, firstInvalid: 'label_values(node_filesystem_avail_bytes, device)' } },
            { dimension: 'completion', score: 1, detail: { status: 'succeeded' } },
            { dimension: 'feedback', score: 0, detail: { rating: 'down', reasons: ['wrong_conclusion'] } },
          ], now),
          targetId: 'message-disk-guess',
        },
        evaluated('message-disk-guess', 0.25, 'Guesses the WAL without reading any disk metric.', 2),
      ],
    },
  ];
}

function seedReviews(now: number): Review[] {
  return [
    {
      id: 'review-disk',
      targetKind: 'chat_answer',
      targetId: 'message-disk-guess',
      source: 'team_chat',
      cause: 'thumbs_down',
      score: 0.25,
      status: 'pending',
      note: '',
      createdAt: now - 25 * 60 * MINUTE,
      updatedAt: now - 25 * 60 * MINUTE,
    },
    {
      id: 'review-health',
      targetKind: 'chat_answer',
      targetId: 'message-health-report',
      source: 'automation',
      cause: 'sampled',
      score: 0.83,
      status: 'pending',
      note: '',
      createdAt: now - 9 * 60 * MINUTE,
      updatedAt: now - 9 * 60 * MINUTE,
    },
  ];
}

function seedEvaluator(now: number): Evaluator {
  return {
    id: EVALUATOR_ID,
    name: 'Answer accuracy',
    rubric: 'Score how well the answer finds and explains what is wrong, from 1 to 5.',
    targets: ['team_chat', 'automation'],
    samplePercent: 20,
    providerId: null,
    model: null,
    temperature: 0,
    enabled: true,
    language: 'en-us',
    createdBy: 'dev',
    updatedBy: 'dev',
    createdAt: now - 3 * DAY,
    updatedAt: now - 3 * DAY,
  };
}

function overviewOf(now: number, days: number) {
  const today = Math.floor(now / DAY);
  const daily = Array.from({ length: 7 }, (_, index) => ({
    day: today - 6 + index,
    answers: 4 + index,
    rulesMean: 0.8 + index * 0.02,
    evaluated: index % 2 === 0 ? 1 : 2,
    evaluatorMean: 0.62 + index * 0.03,
  }));

  return {
    days,
    overview: {
      answers: 42,
      rulesMean: 0.86,
      evaluated: 12,
      evaluatorMean: 0.71,
      thumbsUp: 9,
      thumbsDown: 3,
      pendingReviews: 2,
      daily,
      dimensions: [
        { dimension: 'tool_success', mean: 0.91, answers: 38 },
        { dimension: 'evidence', mean: 0.74, answers: 30 },
        { dimension: 'query_validity', mean: 0.88, answers: 27 },
        { dimension: 'answer_format', mean: 0.97, answers: 40 },
        { dimension: 'completion', mean: 0.95, answers: 42 },
        { dimension: 'feedback', mean: 0.75, answers: 12 },
      ],
      sources: [
        { key: 'team_chat', answers: 24, rulesMean: 0.84, evaluated: 8, evaluatorMean: 0.68 },
        { key: 'personal_chat', answers: 11, rulesMean: 0.9, evaluated: 0, evaluatorMean: null },
        { key: 'automation', answers: 5, rulesMean: 0.87, evaluated: 4, evaluatorMean: 0.77 },
        { key: 'incident_rca', answers: 2, rulesMean: 1, evaluated: 0, evaluatorMean: null },
      ],
      prompts: [
        { key: 'prompt-system-default', version: '1', answers: 36, rulesMean: 0.86, evaluated: 11, evaluatorMean: 0.7 },
        { key: 'analysis.root_cause', answers: 2, rulesMean: 1, evaluated: 0, evaluatorMean: null },
      ],
      skills: [{ key: 'insight', version: '1.0.0', answers: 3, rulesMean: 0.92, evaluated: 1, evaluatorMean: 0.75 }],
      models: [{ key: 'gpt-4.1', answers: 42, rulesMean: 0.86, evaluated: 12, evaluatorMean: 0.71 }],
    },
    promptNames: { 'prompt-system-default': 'System default', 'analysis.root_cause': 'Root-cause investigation' },
    evaluators: [{ id: EVALUATOR_ID, name: 'Answer accuracy', enabled: true }],
  };
}

/** How a check of a prompt goes: queued when started, done with a verdict once read twice. */
interface CheckState {
  check: PromptCheck;
  reads: number;
}

export function registerQualityRoutes(
  app: Express,
  now: number,
  findPrompt: (id: string) => Record<string, unknown> | undefined,
) {
  const answers = seedAnswers(now);
  const reviews = seedReviews(now);
  const evaluators: Evaluator[] = [seedEvaluator(now)];
  let settings: ScoringSettings = { reviewThreshold: 0.6, reviewSamplePercent: 0, gate: 'warn' };
  const checks = new Map<string, CheckState>();
  const evaluatorNames = () =>
    evaluators.map((evaluator) => ({ id: evaluator.id, name: evaluator.name, enabled: evaluator.enabled }));
  const subjectOf = (review: Review) => {
    const answer = answers.find((item) => item.targetId === review.targetId);
    return answer ? { ...answer, scores: undefined } : null;
  };

  app.get('/api/v1/agent/quality/overview', (req, res) => res.json(overviewOf(now, Number(req.query.days ?? 30))));

  app.get('/api/v1/agent/quality/answers', (req, res) => {
    const below = req.query.below === undefined ? null : Number(req.query.below);
    const source = req.query.source ? String(req.query.source) : null;
    const listed = answers.filter((answer) => {
      if (source && answer.source !== source) return false;
      if (below === null) return true;
      const evaluated = answer.scores.filter((score) => score.scorer !== 'rules');
      const overall = evaluated.length
        ? evaluated.reduce((sum, score) => sum + score.score, 0) / evaluated.length
        : (answer.scores[0]?.score ?? 1);
      return overall < below;
    });
    res.json({ days: Number(req.query.days ?? 30), answers: listed, next: null, evaluators: evaluatorNames() });
  });

  app.get('/api/v1/agent/quality/answers/:kind/:id', (req, res) => {
    const answer = answers.find((item) => item.targetKind === req.params.kind && item.targetId === req.params.id);
    if (!answer) return res.status(404).json({ error: 'not_found', message: 'not found' });
    const review = reviews.find((item) => item.targetId === answer.targetId) ?? null;
    return res.json({
      answer,
      review,
      material: {
        question: answer.question,
        answer:
          answer.targetKind === 'incident_rca'
            ? answer.answer
            : JSON.stringify({
                summary: answer.answer,
                evidence: [{ label: 'p95 of checkout-api', kind: 'metrics' }],
                likely_causes: ['The payment connection pool is too small'],
              }),
        tools: [
          { name: 'query_metrics', failed: false },
          { name: 'query_logs', failed: answer.targetId === 'message-disk-guess' },
        ],
      },
      evaluators: evaluatorNames(),
    });
  });

  app.get('/api/v1/agent/quality/reviews', (req, res) => {
    const status = String(req.query.status ?? 'pending');
    const listed = reviews.filter((review) => status === 'all' || review.status === status);
    res.json({ reviews: listed.map((review) => ({ ...review, subject: subjectOf(review) })), next: null });
  });

  app.post('/api/v1/agent/quality/reviews/:id/decision', (req, res) => {
    const review = reviews.find((item) => item.id === req.params.id);
    if (!review) return res.status(404).json({ error: 'not_found', message: 'not found' });
    Object.assign(review, {
      status: req.body?.status,
      note: req.body?.note ?? '',
      decidedBy: 'dev',
      decidedAt: now,
      updatedAt: now,
    });
    return res.json({ review });
  });

  app.get('/api/v1/agent/quality/evaluators', (_req, res) => res.json({ evaluators }));

  app.post('/api/v1/agent/quality/evaluators', (req, res) => {
    const evaluator: Evaluator = {
      ...seedEvaluator(now),
      ...req.body,
      id: `evaluator-${evaluators.length + 1}`,
      createdAt: now,
      updatedAt: now,
    };
    evaluators.push(evaluator);
    res.json(evaluator);
  });

  app.put('/api/v1/agent/quality/evaluators/:id', (req, res) => {
    const evaluator = evaluators.find((item) => item.id === req.params.id);
    if (!evaluator) return res.status(404).json({ error: 'not_found', message: 'not found' });
    Object.assign(evaluator, req.body, { updatedAt: now });
    return res.json(evaluator);
  });

  app.delete('/api/v1/agent/quality/evaluators/:id', (req, res) => {
    const index = evaluators.findIndex((item) => item.id === req.params.id);
    if (index >= 0) evaluators.splice(index, 1);
    res.json({ deleted: true });
  });

  app.get('/api/v1/agent/quality/settings', (_req, res) => res.json(settings));

  app.put('/api/v1/agent/quality/settings', (req, res) => {
    settings = { ...settings, ...req.body, updatedBy: 'dev', updatedAt: now };
    res.json(settings);
  });

  app.get('/api/v1/agent/settings/prompts/:id/gate', (req, res) => {
    const prompt = findPrompt(req.params.id);
    if (!prompt) return res.status(404).json({ error: 'not_found', message: 'not found' });
    const state = checks.get(req.params.id);

    if (state && state.check.status !== 'done') {
      state.reads += 1;
      if (state.reads >= 2) {
        state.check = {
          ...state.check,
          status: 'done',
          samples: state.check.samples.map((sample, index) => ({
            ...sample,
            baseline: 0.75,
            candidate: index === 0 ? 1 : 0.75,
            candidateReason: index === 0 ? 'Names the exhausted pool and when it started.' : '',
          })),
          baselineMean: 0.75,
          candidateMean: 0.83,
          verdict: 'better',
          finishedAt: now,
        };
      }
    }

    const check = state?.check ?? null;
    const done = check?.status === 'done';
    return res.json({
      mode: settings.gate,
      checked: prompt.purpose === 'system' || prompt.purpose === 'root_cause',
      decision: done ? 'allow' : 'confirm',
      reason: done ? undefined : check ? 'prompt_check_running' : 'prompt_check_required',
      check,
      current: Boolean(check),
      defaultPromptId: 'prompt-system-default',
    });
  });

  app.post('/api/v1/agent/settings/prompts/:id/checks', (req, res) => {
    const prompt = findPrompt(req.params.id);
    if (!prompt) return res.status(404).json({ error: 'not_found', message: 'not found' });
    const sample = (targetId: string, regression: boolean) => ({
      targetKind: 'chat_answer' as const,
      targetId: targetId,
      regression,
      baselineReason: '',
      candidateReason: '',
    });
    const check: PromptCheck = {
      id: `check-${req.params.id}`,
      promptId: req.params.id,
      promptVersion: Number(prompt.version ?? 1),
      purpose: String(prompt.purpose),
      baselinePromptId: 'prompt-system-default',
      baselineVersion: 1,
      evaluatorId: EVALUATOR_ID,
      status: 'running',
      samples: [
        sample('message-disk-guess', true),
        sample('message-checkout-latency', false),
        sample('message-health-report', false),
      ],
      tokens: 0,
      requestedBy: 'dev',
      createdAt: now,
    };
    checks.set(req.params.id, { check, reads: 0 });
    return res.json(check);
  });
}

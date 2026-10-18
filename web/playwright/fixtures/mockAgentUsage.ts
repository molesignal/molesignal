/**
 * Mock Mole Agent usage, budget and feedback.
 *
 * Today (the mock session's frozen clock, a UTC day) has 41,236 tokens across
 * chat, root cause analysis and log inspections; three earlier days have some
 * too. The instance default is 200,000 tokens a day and the organization has
 * no limit of its own, so the budget starts well clear of its limit. A test
 * that lowers the limit on the Usage page moves the chat into a warning, a
 * past-the-budget answer or a refusal, the way the backend would. Three of
 * today's chat calls and the LLM functions three days ago went to a model
 * provider without prices, so those days' costs are only a floor.
 *
 * Feedback is kept per test: what one test rates is gone with it.
 */
import type { Express } from 'express';

import type { AgentFeedback, FeedbackInput } from '../../src/api/agent/feedback';
import type {
  AgentUsage,
  BudgetLevel,
  DailyUsageRow,
  InteractiveBudgetMode,
} from '../../src/api/agent/usage';

const DAY = 86_400_000_000;
const WARNING_PERCENT = 80;

export const MOCK_USAGE_INSTANCE_DEFAULT = 200_000;
export const MOCK_USAGE_TODAY_TOKENS = 41_236;

function seedDays(today: number): DailyUsageRow[] {
  const row = (
    daysAgo: number,
    origin: DailyUsageRow['origin'],
    calls: number,
    prompt: number,
    completion: number,
    cost: number,
    unpriced = 0,
  ): DailyUsageRow => ({
    day_start_micros: today - daysAgo * DAY,
    origin,
    calls,
    unpriced_calls: unpriced,
    prompt_tokens: prompt,
    completion_tokens: completion,
    cost,
    currency: cost > 0 ? 'USD' : null,
  });

  return [
    row(0, 'chat', 14, 26_100, 4_312, 0.11, 3),
    row(0, 'rca', 3, 7_000, 1_200, 0.03),
    row(0, 'inspection', 4, 2_200, 424, 0.01),
    row(1, 'chat', 9, 18_000, 2_900, 0.07),
    row(1, 'inspection', 4, 2_150, 410, 0.01),
    row(3, 'chat', 2, 3_400, 600, 0.01),
    row(3, 'llm_eval', 40, 6_000, 1_800, 0, 40),
  ];
}

export interface MockAgentUsage {
  /** What the chat says before its answer: close to or past the limit. */
  notice(): Record<string, unknown> | null;
  /**
   * The 429 body a chat or an on-demand root cause analysis gets when the
   * budget is used up and they are blocked.
   */
  refusal(): Record<string, unknown> | null;
}

export function registerAgentUsageRoutes(app: Express, nowIso: string): MockAgentUsage {
  const nowMicros = Date.parse(nowIso) * 1000;
  const today = Math.floor(nowMicros / DAY) * DAY;
  const days = seedDays(today);
  let orgLimit: number | null = null;
  let interactiveMode: InteractiveBudgetMode = 'block';
  let updatedAt: number | null = null;

  const effectiveLimit = () => orgLimit ?? MOCK_USAGE_INSTANCE_DEFAULT;
  const usedToday = () =>
    days
      .filter((row) => row.day_start_micros === today)
      .reduce((sum, row) => sum + row.prompt_tokens + row.completion_tokens, 0);

  const level = (): BudgetLevel => {
    const limit = effectiveLimit();
    const used = usedToday();
    if (limit <= 0) return 'unlimited';
    if (used >= limit) return 'exhausted';
    if (used * 100 >= limit * WARNING_PERCENT) return 'warning';
    return 'ok';
  };

  const view = (): AgentUsage => ({
    status: {
      level: level(),
      used_tokens: usedToday(),
      limit_tokens: Math.max(0, effectiveLimit()),
      warning_percent: WARNING_PERCENT,
      interactive_mode: interactiveMode,
      resets_at: today + DAY,
    },
    budget: {
      org_limit: orgLimit,
      instance_default: MOCK_USAGE_INSTANCE_DEFAULT,
      effective_limit: effectiveLimit(),
      interactive_mode: interactiveMode,
      warning_percent: WARNING_PERCENT,
      updated_by: updatedAt === null ? null : 'u1',
      updated_at: updatedAt,
    },
    cost_known: true,
    days: days.map((row) => ({ ...row })),
  });

  app.get('/api/v1/agent/usage', (_req, res) => res.json(view()));
  app.put('/api/v1/agent/usage/budget', (req, res) => {
    const limit = req.body?.daily_token_limit;
    if (limit !== null && (typeof limit !== 'number' || limit < 0)) {
      return res.status(400).json({ error: 'daily_token_limit must be zero or more' });
    }
    orgLimit = limit;
    interactiveMode = req.body?.interactive_mode === 'warn' ? 'warn' : 'block';
    updatedAt = nowMicros;
    return res.json(view());
  });

  const feedback = new Map<string, AgentFeedback>();
  const key = (kind: string, id: string) => `${kind}:${id}`;

  app.get('/api/v1/agent/feedback', (req, res) => {
    const kind = String(req.query.target_kind ?? '');
    const ids = String(req.query.target_ids ?? '')
      .split(',')
      .map((id) => id.trim())
      .filter(Boolean);
    res.json({
      feedback: ids
        .map((id) => feedback.get(key(kind, id)))
        .filter((item): item is AgentFeedback => Boolean(item)),
    });
  });
  app.put('/api/v1/agent/feedback', (req, res) => {
    const input = req.body as FeedbackInput;
    if (input.target_kind === 'chat_message' && !input.chat_id) {
      return res.status(400).json({ error: 'chat_id is required to rate a chat answer' });
    }
    const existing = feedback.get(key(input.target_kind, input.target_id));
    const saved: AgentFeedback = {
      id: existing?.id ?? `feedback-${feedback.size + 1}`,
      target_kind: input.target_kind,
      target_id: input.target_id,
      rating: input.rating,
      reasons: input.rating === 'down' ? [...new Set(input.reasons ?? [])] : [],
      comment: (input.comment ?? '').trim(),
      created_at: existing?.created_at ?? nowMicros,
      updated_at: nowMicros,
    };
    feedback.set(key(input.target_kind, input.target_id), saved);
    return res.json(saved);
  });
  app.delete('/api/v1/agent/feedback/:kind/:id', (req, res) => {
    const removed = feedback.delete(key(req.params.kind, req.params.id));
    res.json({ removed });
  });

  const numbers = () => ({
    used_tokens: usedToday(),
    limit_tokens: Math.max(0, effectiveLimit()),
    resets_at_micros: today + DAY,
  });

  return {
    notice: () => {
      const current = level();
      if (current !== 'warning' && current !== 'exhausted') return null;
      return { kind: 'budget', level: current, ...numbers() };
    },
    refusal: () =>
      level() === 'exhausted' && interactiveMode === 'block'
        ? {
            error: 'agent_budget_exhausted',
            code: 'agent_budget_exhausted',
            message: "the organization's daily Mole Agent token budget is used up",
            ...numbers(),
          }
        : null,
  };
}

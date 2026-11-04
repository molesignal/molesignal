import { http } from '@/lib/http';

/** What is scored: an answer in a chat (an automation's runs too), or an incident's RCA. */
export type ScoreTargetKind = 'chat_answer' | 'incident_rca';

/** Where a scored answer comes from. Personal chats only appear in the totals. */
export type ScoreSource = 'personal_chat' | 'team_chat' | 'automation' | 'incident_rca';

/** The sources an evaluator can score: personal chats are scored by rules only. */
export const EVALUATED_SOURCES: readonly ScoreSource[] = ['team_chat', 'automation', 'incident_rca'];

export type RuleDimension =
  | 'tool_success'
  | 'evidence'
  | 'query_validity'
  | 'answer_format'
  | 'completion'
  | 'feedback';

export const RULE_DIMENSIONS: readonly RuleDimension[] = [
  'tool_success',
  'evidence',
  'query_validity',
  'answer_format',
  'completion',
  'feedback',
];

/** One rule dimension with the facts it was worked out from. */
export interface DimensionScore {
  dimension: RuleDimension;
  /** 0 to 1. */
  score: number;
  detail: Record<string, unknown>;
}

export interface ScoreAttribution {
  chatId?: string;
  teamId?: string;
  automationId?: string;
  userId?: string;
  promptTemplateId?: string;
  promptVersion?: number;
  promptKey?: string;
  providerId?: string;
  model?: string;
  skill?: string;
  skillVersion?: string;
}

/** One scorer's score of one answer: `rules`, or an evaluator's ID. */
export interface Score {
  id: string;
  targetKind: ScoreTargetKind;
  targetId: string;
  scorer: string;
  /** 0 to 1. */
  score: number;
  dimensions: DimensionScore[];
  /** The evaluator's reason; empty for rules. */
  reason: string;
  /** How many times an evaluator scored it: a low score is scored again. */
  samples: number;
  evaluatorModel?: string;
  createdAt: number;
  updatedAt: number;
}

/** A scored answer with every score it has, rules first. */
export interface ScoredAnswer {
  targetKind: ScoreTargetKind;
  targetId: string;
  source: ScoreSource;
  /** The start of the question and of the answer. */
  question: string;
  answer: string;
  attribution: ScoreAttribution;
  targetAt: number;
  scores: Score[];
}

export interface EvaluatorName {
  id: string;
  name: string;
  enabled: boolean;
}

export interface DailyQuality {
  /** Days since 1970-01-01 (UTC). */
  day: number;
  answers: number;
  rulesMean: number | null;
  evaluated: number;
  evaluatorMean: number | null;
}

export interface GroupQuality {
  key: string;
  version?: string;
  answers: number;
  rulesMean: number | null;
  evaluated: number;
  evaluatorMean: number | null;
}

export interface QualityOverview {
  answers: number;
  rulesMean: number | null;
  evaluated: number;
  evaluatorMean: number | null;
  thumbsUp: number;
  thumbsDown: number;
  pendingReviews: number;
  daily: DailyQuality[];
  dimensions: Array<{ dimension: RuleDimension; mean: number; answers: number }>;
  sources: GroupQuality[];
  prompts: GroupQuality[];
  skills: GroupQuality[];
  models: GroupQuality[];
}

export interface OverviewResponse {
  days: number;
  overview: QualityOverview;
  /** Prompt names by ID, and built-in prompts also by their key. */
  promptNames: Record<string, string>;
  evaluators: EvaluatorName[];
}

export interface Cursor {
  beforeAt: number;
  beforeId: string;
}

export interface AnswersResponse {
  days: number;
  answers: ScoredAnswer[];
  next: Cursor | null;
  evaluators: EvaluatorName[];
}

export type ReviewCause = 'low_score' | 'thumbs_down' | 'sampled';
export type ReviewStatus = 'pending' | 'accepted' | 'rejected';

export interface Review {
  id: string;
  targetKind: ScoreTargetKind;
  targetId: string;
  source: ScoreSource;
  cause: ReviewCause;
  score?: number;
  status: ReviewStatus;
  note: string;
  decidedBy?: string;
  decidedAt?: number;
  createdAt: number;
  updatedAt: number;
}

export interface ReviewItem extends Review {
  /** What was scored; gone when its scores were removed. */
  subject: Omit<ScoredAnswer, 'scores'> | null;
}

export interface AnswerMaterial {
  question: string;
  /** The answer as it was written: the structured answer of a chat, or the Markdown of an RCA. */
  answer: string;
  tools: Array<{ name: string; failed: boolean }>;
}

export interface AnswerDetail {
  answer: ScoredAnswer;
  review: Review | null;
  /** Missing when the chat or the RCA is gone. */
  material: AnswerMaterial | null;
  evaluators: EvaluatorName[];
}

export interface Evaluator {
  id: string;
  name: string;
  rubric: string;
  targets: ScoreSource[];
  samplePercent: number;
  providerId: string | null;
  model: string | null;
  temperature: number;
  enabled: boolean;
  /** The interface language of whoever saved it last: the evaluator writes its reasons in it. */
  language: string;
  createdBy: string;
  updatedBy: string;
  createdAt: number;
  updatedAt: number;
}

export interface EvaluatorInput {
  name: string;
  rubric: string;
  targets: ScoreSource[];
  samplePercent: number;
  providerId?: string | null;
  model?: string | null;
  temperature?: number;
  enabled: boolean;
}

export type GateMode = 'off' | 'warn' | 'block';

export interface ScoringSettings {
  /** Answers scoring below it (0 to 1) wait for review. */
  reviewThreshold: number;
  reviewSamplePercent: number;
  gate: GateMode;
  updatedBy?: string;
  updatedAt?: number;
}

export type CheckStatus = 'queued' | 'running' | 'done' | 'failed';
export type CheckVerdict = 'better' | 'same' | 'worse' | 'inconclusive';

export interface CheckSample {
  targetKind: ScoreTargetKind;
  targetId: string;
  regression: boolean;
  baseline?: number;
  candidate?: number;
  baselineReason: string;
  candidateReason: string;
  error?: string;
}

export interface PromptCheck {
  id: string;
  promptId: string;
  promptVersion: number;
  purpose: string;
  baselinePromptId: string;
  baselineVersion: number;
  evaluatorId: string;
  status: CheckStatus;
  samples: CheckSample[];
  baselineMean?: number;
  candidateMean?: number;
  verdict?: CheckVerdict;
  error?: string;
  tokens: number;
  requestedBy: string;
  createdAt: number;
  finishedAt?: number;
}

export type GateReason =
  | 'prompt_check_required'
  | 'prompt_check_running'
  | 'prompt_check_worse'
  | 'prompt_check_unavailable';

/** What happens when a prompt is set as the default, and the last check of it. */
export interface PromptGate {
  mode: GateMode;
  /** Prompts of this purpose are checked before they become the default. */
  checked: boolean;
  decision: 'allow' | 'confirm' | 'refuse';
  reason?: GateReason;
  check: PromptCheck | null;
  /** The check is of the prompt's present version against the present default. */
  current: boolean;
  defaultPromptId: string;
}

export interface QualityFilter {
  days: number;
  source?: ScoreSource | undefined;
}

export async function overview(filter: QualityFilter): Promise<OverviewResponse> {
  const { data } = await http.get<OverviewResponse>('/agent/quality/overview', {
    params: { days: filter.days, source: filter.source },
  });
  return data;
}

export async function listAnswers(
  filter: QualityFilter & { below?: number | undefined; cursor?: Cursor | null | undefined },
): Promise<AnswersResponse> {
  const { data } = await http.get<AnswersResponse>('/agent/quality/answers', {
    params: {
      days: filter.days,
      source: filter.source,
      below: filter.below,
      beforeAt: filter.cursor?.beforeAt,
      beforeId: filter.cursor?.beforeId,
    },
  });
  return data;
}

export async function getAnswer(kind: ScoreTargetKind, id: string): Promise<AnswerDetail> {
  const { data } = await http.get<AnswerDetail>(
    `/agent/quality/answers/${encodeURIComponent(kind)}/${encodeURIComponent(id)}`,
  );
  return data;
}

export async function listReviews(
  status: ReviewStatus | 'all',
  cursor?: Cursor | null,
): Promise<{ reviews: ReviewItem[]; next: Cursor | null }> {
  const { data } = await http.get<{ reviews: ReviewItem[]; next: Cursor | null }>(
    '/agent/quality/reviews',
    { params: { status, beforeAt: cursor?.beforeAt, beforeId: cursor?.beforeId } },
  );
  return data;
}

export async function decideReview(
  id: string,
  status: Exclude<ReviewStatus, 'pending'>,
  note: string,
): Promise<Review> {
  const { data } = await http.post<{ review: Review }>(
    `/agent/quality/reviews/${encodeURIComponent(id)}/decision`,
    { status, note },
  );
  return data.review;
}

export async function listEvaluators(): Promise<Evaluator[]> {
  const { data } = await http.get<{ evaluators: Evaluator[] }>('/agent/quality/evaluators');
  return data.evaluators ?? [];
}

export async function createEvaluator(input: EvaluatorInput): Promise<Evaluator> {
  const { data } = await http.post<Evaluator>('/agent/quality/evaluators', input);
  return data;
}

export async function updateEvaluator(id: string, input: EvaluatorInput): Promise<Evaluator> {
  const { data } = await http.put<Evaluator>(
    `/agent/quality/evaluators/${encodeURIComponent(id)}`,
    input,
  );
  return data;
}

export async function deleteEvaluator(id: string): Promise<void> {
  await http.delete(`/agent/quality/evaluators/${encodeURIComponent(id)}`);
}

export async function getSettings(): Promise<ScoringSettings> {
  const { data } = await http.get<ScoringSettings>('/agent/quality/settings');
  return data;
}

export async function putSettings(
  input: Pick<ScoringSettings, 'reviewThreshold' | 'reviewSamplePercent' | 'gate'>,
): Promise<ScoringSettings> {
  const { data } = await http.put<ScoringSettings>('/agent/quality/settings', input);
  return data;
}

export async function promptGate(promptId: string): Promise<PromptGate> {
  const { data } = await http.get<PromptGate>(
    `/agent/settings/prompts/${encodeURIComponent(promptId)}/gate`,
  );
  return data;
}

export async function startPromptCheck(promptId: string): Promise<PromptCheck> {
  const { data } = await http.post<PromptCheck>(
    `/agent/settings/prompts/${encodeURIComponent(promptId)}/checks`,
  );
  return data;
}

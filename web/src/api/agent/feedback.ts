import { http } from '@/lib/http';

export type FeedbackTargetKind = 'chat_message' | 'incident_rca';

export type FeedbackRating = 'up' | 'down';

export type FeedbackReason =
  | 'wrong_conclusion'
  | 'missed_cause'
  | 'unusable_advice'
  | 'off_topic'
  | 'other';

export const FEEDBACK_REASONS: readonly FeedbackReason[] = [
  'wrong_conclusion',
  'missed_cause',
  'unusable_advice',
  'off_topic',
  'other',
];

export interface AgentFeedback {
  id: string;
  target_kind: FeedbackTargetKind;
  target_id: string;
  rating: FeedbackRating;
  reasons: FeedbackReason[];
  comment: string;
  created_at: number;
  updated_at: number;
}

export interface FeedbackInput {
  target_kind: FeedbackTargetKind;
  target_id: string;
  /** The chat an answer belongs to; required for chat answers. */
  chat_id?: string | undefined;
  rating: FeedbackRating;
  reasons?: FeedbackReason[] | undefined;
  comment?: string | undefined;
}

/** The signed-in person's own ratings of these targets. */
export async function listFeedback(
  targetKind: FeedbackTargetKind,
  targetIds: readonly string[],
): Promise<AgentFeedback[]> {
  if (targetIds.length === 0) return [];
  const { data } = await http.get<{ feedback: AgentFeedback[] }>('/agent/feedback', {
    params: { target_kind: targetKind, target_ids: targetIds.join(',') },
  });
  return data.feedback ?? [];
}

export async function putFeedback(input: FeedbackInput): Promise<AgentFeedback> {
  const { data } = await http.put<AgentFeedback>('/agent/feedback', input);
  return data;
}

export async function deleteFeedback(
  targetKind: FeedbackTargetKind,
  targetId: string,
): Promise<void> {
  await http.delete(
    `/agent/feedback/${encodeURIComponent(targetKind)}/${encodeURIComponent(targetId)}`,
  );
}

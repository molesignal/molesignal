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
  targetKind: FeedbackTargetKind;
  targetId: string;
  rating: FeedbackRating;
  reasons: FeedbackReason[];
  comment: string;
  createdAt: number;
  updatedAt: number;
}

export interface FeedbackInput {
  targetKind: FeedbackTargetKind;
  targetId: string;
  /** The chat an answer belongs to; required for chat answers. */
  chatId?: string | undefined;
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
    params: { targetKind: targetKind, targetIds: targetIds.join(',') },
  });
  return data.feedback ?? [];
}

export async function putFeedback(input: FeedbackInput): Promise<AgentFeedback> {
  const { data } = await http.put<AgentFeedback>('/agent/feedback', input);
  return data;
}

/** Withdraw a rating. `chatId` lets a chat answer be scored again without it. */
export async function deleteFeedback(
  targetKind: FeedbackTargetKind,
  targetId: string,
  chatId?: string,
): Promise<void> {
  await http.delete(
    `/agent/feedback/${encodeURIComponent(targetKind)}/${encodeURIComponent(targetId)}`,
    chatId ? { params: { chatId: chatId } } : undefined,
  );
}

import { http } from '@/lib/http';
import type { Urgency } from '@/types/alerting';

import type { NotifyCategory } from './index';

export interface UserNotifyPreferenceStep {
  id: string;
  preferenceId: string;
  endpointId: string;
  stepOrder: number;
  /** Seconds after the person was first notified that this step is used if nobody acknowledged. */
  delaySecs: number;
  /** Which of the two step lists it is in; only alert and escalation have a low one. */
  urgency?: Urgency;
  /** How the IM app pages harder once this step's message is sent (`app`, `sms`, `phone`); none when null. */
  boost?: string | null;
  createdAt: number;
}

export interface UserNotifyPreference {
  id: string;
  organizationId: string;
  userId: string;
  category: NotifyCategory;
  enabled: boolean;
  quietHours?: Record<string, unknown> | null;
  /** High-urgency alerts still come during quiet hours; low-urgency ones never do. */
  allowHighUrgencyBypass: boolean;
  steps: UserNotifyPreferenceStep[];
  createdAt: number;
  updatedAt: number;
}

export interface UserNotifyPreferenceInput {
  enabled: boolean;
  /** The steps in order. An endpoint comes twice only with a different boost on each step. */
  endpointIds: string[];
  /** One per endpoint, in the same order; left out, no step waits. */
  stepDelaysSecs?: number[];
  /** One per endpoint, in the same order: how the IM app pages harder after the step, `null` for not at all. */
  stepBoosts?: Array<string | null>;
  /** The low-urgency steps (alert and escalation only); left empty, low urgency uses the first step above. */
  lowEndpointIds?: string[];
  lowStepDelaysSecs?: number[];
  lowStepBoosts?: Array<string | null>;
  quietHours?: Record<string, unknown> | null;
  allowHighUrgencyBypass: boolean;
}

export async function listPreferences(userId: string): Promise<UserNotifyPreference[]> {
  const { data } = await http.get<UserNotifyPreference[]>(
    `/users/${userId}/notify-preferences`,
  );
  return data;
}

export async function updatePreference(
  userId: string,
  category: NotifyCategory,
  input: UserNotifyPreferenceInput,
): Promise<UserNotifyPreference> {
  const { data } = await http.put<UserNotifyPreference>(
    `/users/${userId}/notify-preferences/${category}`,
    input,
  );
  return data;
}

import type {
  EscalationPolicy,
  Incident,
  Schedule,
  ScheduleOverride,
} from '@/types/alerting';

import {
  nextScheduleBoundary,
  replacedUserAt,
  resolutionStartedAt,
  resolveScheduleAt,
  scheduleStatus,
  type OnCallResolution,
  type ScheduleStatus,
} from '../../alerts/schedule/model';

export interface FeaturedOnCall {
  schedule: Schedule;
  status: ScheduleStatus;
  current: OnCallResolution | null;
  nextAt: number | null;
  currentStartedAt: number | null;
  nextUserId: string | null;
  activeOverride: ScheduleOverride | null;
  replacedUserId: string | null;
  isMine: boolean;
}

export interface OnCallShiftOverview {
  incidentCount: number;
  pendingCount: number;
  acknowledgedCount: number;
  escalatedCount: number;
  escalationPolicyNames: string[];
}

function activeOverrideAt(
  schedule: Schedule,
  atMicros: number,
): ScheduleOverride | null {
  return (
    schedule.overrides.find(
      (override) =>
        override.startAt <= atMicros && atMicros < override.endAt,
    ) ?? null
  );
}

function priority(feature: FeaturedOnCall): number {
  if (feature.status === 'gap') return 0;
  if (feature.isMine) return 1;
  if (feature.activeOverride) return 2;
  if (feature.status === 'switching') return 3;
  if (feature.status === 'active') return 4;
  if (feature.status === 'not_started') return 5;
  return 6;
}

export function selectFeaturedOnCall(
  schedules: Schedule[],
  currentUserId: string,
  nowMicros: number,
): FeaturedOnCall | null {
  const features = schedules.map<FeaturedOnCall>((schedule) => {
    const status = scheduleStatus(schedule, nowMicros);
    const current = resolveScheduleAt(schedule, nowMicros);
    const nextAt = nextScheduleBoundary(schedule, nowMicros);
    const next =
      nextAt == null
        ? null
        : resolveScheduleAt(schedule, nextAt + 1);
    const activeOverride = activeOverrideAt(schedule, nowMicros);

    return {
      schedule,
      status,
      current,
      nextAt,
      currentStartedAt: resolutionStartedAt(schedule, current, nowMicros),
      nextUserId: next?.userId ?? null,
      activeOverride,
      replacedUserId:
        activeOverride == null ? null : replacedUserAt(schedule, nowMicros),
      isMine:
        Boolean(currentUserId) && current?.userId === currentUserId,
    };
  });

  return (
    features.sort(
      (left, right) =>
        priority(left) - priority(right) ||
        left.schedule.name.localeCompare(right.schedule.name),
    )[0] ?? null
  );
}

function policyTargetsSchedule(
  policy: EscalationPolicy,
  scheduleId: string,
): boolean {
  return policy.steps.some((step) =>
    step.targets.some(
      (target) =>
        target.kind === 'schedule' &&
        target.scheduleId === scheduleId,
    ),
  );
}

export function summarizeOnCallShift(
  feature: FeaturedOnCall,
  incidents: readonly Incident[],
  policies: readonly EscalationPolicy[],
): OnCallShiftOverview {
  const relatedPolicies = policies.filter((policy) =>
    policyTargetsSchedule(policy, feature.schedule.id),
  );
  const relatedPolicyIds = new Set(
    relatedPolicies.map((policy) => policy.id),
  );
  const startedAt = feature.currentStartedAt;
  const endedAt = feature.nextAt ?? Number.POSITIVE_INFINITY;

  const shiftIncidents =
    startedAt == null
      ? []
      : incidents.filter((incident) => {
          if (
            incident.createdAt < startedAt ||
            incident.createdAt >= endedAt
          ) {
            return false;
          }
          if (relatedPolicyIds.size > 0) {
            return relatedPolicyIds.has(incident.escalationPolicyId);
          }
          return feature.current
            ? incident.assignees.includes(feature.current.userId)
            : false;
        });

  return {
    incidentCount: shiftIncidents.length,
    pendingCount: shiftIncidents.filter(
      (incident) => incident.status === 'open',
    ).length,
    acknowledgedCount: shiftIncidents.filter(
      (incident) => incident.acknowledgedAt != null,
    ).length,
    escalatedCount: shiftIncidents.filter(
      (incident) =>
        incident.currentStep > 0 || incident.currentLoop > 0,
    ).length,
    escalationPolicyNames: relatedPolicies.map((policy) => policy.name),
  };
}

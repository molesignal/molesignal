import type { StatusPageIncident } from '@/api/statusPages';

export interface PrimaryEventSubmitAction {
  mode: 'publish' | 'reschedule';
  label: 'publish' | 'save';
}

export function primaryEventSubmitAction(
  event: Pick<StatusPageIncident, 'publicationState' | 'status'> | null,
): PrimaryEventSubmitAction {
  if (event?.publicationState === 'published' && event.status === 'scheduled') {
    return { mode: 'reschedule', label: 'save' };
  }
  return { mode: 'publish', label: 'publish' };
}

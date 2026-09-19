import type { StatusPageIncident } from '@/api/statusPages';

export interface PrimaryEventSubmitAction {
  mode: 'publish' | 'reschedule';
  label: 'publish' | 'save';
}

export function primaryEventSubmitAction(
  event: Pick<StatusPageIncident, 'publication_state' | 'status'> | null,
): PrimaryEventSubmitAction {
  if (event?.publication_state === 'published' && event.status === 'scheduled') {
    return { mode: 'reschedule', label: 'save' };
  }
  return { mode: 'publish', label: 'publish' };
}

import { describe, expect, it } from 'vitest';

import { primaryEventSubmitAction } from './eventSubmission';

describe('primaryEventSubmitAction', () => {
  it('publishes a scheduled maintenance draft', () => {
    expect(primaryEventSubmitAction({
      publicationState: 'draft',
      status: 'scheduled',
    })).toEqual({ mode: 'publish', label: 'publish' });
  });

  it('reschedules maintenance that is already published', () => {
    expect(primaryEventSubmitAction({
      publicationState: 'published',
      status: 'scheduled',
    })).toEqual({ mode: 'reschedule', label: 'save' });
  });

  it('publishes new and non-scheduled events', () => {
    expect(primaryEventSubmitAction(null)).toEqual({ mode: 'publish', label: 'publish' });
    expect(primaryEventSubmitAction({
      publicationState: 'draft',
      status: 'investigating',
    })).toEqual({ mode: 'publish', label: 'publish' });
  });
});

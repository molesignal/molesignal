import { describe, expect, it } from 'vitest';

import { answerFromPayload } from './counts';

describe('answerFromPayload', () => {
  it('reads the bars an endpoint counted', () => {
    expect(
      answerFromPayload({
        buckets: [{ start_micros: 1_000, total: 12, errors: 2 }],
        split: true,
      }),
    ).toEqual({ bars: [{ startMicros: 1_000, total: 12, errors: 2 }], split: true });
    expect(answerFromPayload({ buckets: [], split: false })).toEqual({ bars: [], split: false });
  });

  it('reads why there are no counts', () => {
    expect(answerFromPayload({ buckets: [], split: false, unavailable: 'scan_limit' })).toEqual({
      unavailable: 'scan_limit',
    });
  });

  it('takes an answer that is not counts for a failure', () => {
    // An endpoint the backend does not have, a reason this client does not know.
    expect(answerFromPayload({})).toEqual({ unavailable: 'failed' });
    expect(answerFromPayload(null)).toEqual({ unavailable: 'failed' });
    expect(answerFromPayload({ buckets: [], unavailable: 'something new' })).toEqual({ unavailable: 'failed' });
  });
});

import { describe, expect, it } from 'vitest';

import {
  relatedSignalsFromRecord,
  signalTimeFromTimestamp,
  stringLabelsFromRecord,
} from './relatedSignals';

describe('log related signals', () => {
  it('finds nested trace and service context', () => {
    const record = {
      trace: { id: 'trace-1' },
      service: { name: 'checkout' },
    };
    expect(relatedSignalsFromRecord(record)).toEqual([
      { field: 'trace.id', type: 'trace_id', value: 'trace-1' },
      { field: 'service.name', type: 'service', value: 'checkout' },
    ]);
    expect(stringLabelsFromRecord(record)).toMatchObject({
      'trace.id': 'trace-1',
      'service.name': 'checkout',
    });
  });

  it('scopes a log pivot to ten minutes around the event', () => {
    expect(signalTimeFromTimestamp('2026-08-14T06:00:00.000Z')).toEqual({
      from: '2026-08-14T05:55:00.000Z',
      to: '2026-08-14T06:05:00.000Z',
    });
  });
});

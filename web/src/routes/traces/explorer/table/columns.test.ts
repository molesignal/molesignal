import { describe, expect, it } from 'vitest';

import { DEFAULT_TRACE_COLUMN_IDS, traceColumnKey, traceTableColumns, traceTableTracks } from './columns';
import { moveTraceColumn, showTraceColumn } from './useTraceColumns';

describe('the columns of the trace table', () => {
  it('open with every column a trace carries, in the order the table has always had', () => {
    expect(DEFAULT_TRACE_COLUMN_IDS).toEqual(['_status', '_http', '_start', '_service', '_operation', '_duration', '_spans', '_errors', '_trace']);
    expect(traceColumnKey('_duration')).toBe('duration');
    expect(traceColumnKey('http.route')).toBeNull();
  });

  it('read an arrangement in its order, leaving out a field of the root the stream no longer has', () => {
    const columns = traceTableColumns(['_trace', 'http.route', '_status', 'gone.field'], new Set(['http.route']));

    expect(columns).toEqual([
      { id: '_trace', key: 'trace' },
      { id: 'http.route', key: null },
      { id: '_status', key: 'status' },
    ]);
  });

  it('give the room left to a column that takes it, or to an empty one when none does', () => {
    expect(traceTableTracks(traceTableColumns(['_status', '_service'], new Set()))).toEqual(['52px', 'minmax(96px,0.9fr)']);
    expect(traceTableTracks(traceTableColumns(['_status', '_spans'], new Set()))).toEqual(['52px', '60px', 'minmax(0,1fr)']);
  });

  it('move a column before or after another, and put one shown again back where it opens', () => {
    expect(moveTraceColumn(['_status', '_start', '_service'], '_service', '_status', 'before')).toEqual(['_service', '_status', '_start']);
    expect(moveTraceColumn(['_status', '_start', '_service'], '_status', '_service', 'after')).toEqual(['_start', '_service', '_status']);
    expect(moveTraceColumn(['_status'], '_status', '_status', 'after')).toEqual(['_status']);

    // A column a trace carries goes back before the first of its later columns still shown; a field goes last.
    expect(showTraceColumn(['_status', '_operation', 'http.route'], '_service')).toEqual(['_status', '_service', '_operation', 'http.route']);
    expect(showTraceColumn(['_status', 'http.route'], '_trace')).toEqual(['_status', 'http.route', '_trace']);
    expect(showTraceColumn(['_status'], 'db.system')).toEqual(['_status', 'db.system']);
  });
});

import { describe, expect, it } from 'vitest';

import {
  aggregateTools,
  isNearScrollBottom,
  scrollFadeState,
  shouldPauseAutoScrollForWheel,
  summarizeToolPayload,
} from './chat';

describe('Mole Agent tool strip helpers', () => {
  it('aggregates repeated tool calls and keeps call details', () => {
    const aggregated = aggregateTools([
      {
        id: 'call-1',
        name: 'list_streams',
        status: 'done',
        arguments: '{"org_id":"ignored"}',
        result: '{"streams":["logs"]}',
      },
      {
        id: 'call-2',
        name: 'list_streams',
        status: 'running',
        arguments: '{}',
      },
      {
        id: 'call-3',
        name: 'list_recent_alerts',
        status: 'error',
        result: 'timeout',
      },
    ]);

    expect(aggregated).toMatchObject([
      {
        name: 'list_streams',
        count: 2,
        status: 'running',
        calls: [
          { id: 'call-1', result: '{"streams":["logs"]}' },
          { id: 'call-2', arguments: '{}' },
        ],
      },
      {
        name: 'list_recent_alerts',
        count: 1,
        status: 'error',
        calls: [{ id: 'call-3', result: 'timeout' }],
      },
    ]);
  });

  it('summarizes list_streams results as stream names instead of JSON', () => {
    expect(
      summarizeToolPayload(
        'list_streams',
        JSON.stringify([
          {
            type: 'json',
            json: {
              streams: [
                { name: 'logs1', stream_type: 'logs' },
                { name: 'metrics1', stream_type: 'metrics' },
              ],
            },
          },
        ]),
        'result',
      ),
    ).toEqual(['logs1', 'metrics1']);

    expect(
      summarizeToolPayload('list_streams', '{"streams":["logs/default"]}', 'result'),
    ).toEqual(['logs/default']);
  });

  it('summarizes tool payloads as key text instead of formatted JSON', () => {
    expect(
      summarizeToolPayload('query_logs', '{"sql":"select * from logs","limit":50}', 'arguments'),
    ).toEqual(['sql: select * from logs', 'limit: 50']);
    expect(summarizeToolPayload('query_logs', 'timeout', 'error')).toEqual(['timeout']);
    expect(summarizeToolPayload('query_logs', '', 'result')).toEqual([]);
  });

  it('detects whether the transcript should keep following new output', () => {
    expect(
      isNearScrollBottom({ scrollHeight: 1000, scrollTop: 688, clientHeight: 300 }),
    ).toBe(true);
    expect(
      isNearScrollBottom({ scrollHeight: 1000, scrollTop: 650, clientHeight: 300 }),
    ).toBe(false);
  });

  it('pauses transcript auto-scroll when the user wheels upward', () => {
    expect(shouldPauseAutoScrollForWheel(-1)).toBe(true);
    expect(shouldPauseAutoScrollForWheel(1)).toBe(false);
  });

  it('retracts the scroll-fade at whichever edge the transcript is parked against', () => {
    // overflowing, parked at the very top → only the top edge retracts
    expect(scrollFadeState({ scrollHeight: 1000, scrollTop: 0, clientHeight: 300 })).toEqual({
      top: true,
      bottom: false,
    });
    // overflowing, parked at the bottom → only the bottom edge retracts
    expect(scrollFadeState({ scrollHeight: 1000, scrollTop: 700, clientHeight: 300 })).toEqual({
      top: false,
      bottom: true,
    });
    // overflowing, somewhere in the middle → both edges fade
    expect(scrollFadeState({ scrollHeight: 1000, scrollTop: 400, clientHeight: 300 })).toEqual({
      top: false,
      bottom: false,
    });
    // content shorter than the viewport → nothing to scroll, no fade either side
    expect(scrollFadeState({ scrollHeight: 200, scrollTop: 0, clientHeight: 300 })).toEqual({
      top: true,
      bottom: true,
    });
  });
});

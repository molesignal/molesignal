import { describe, expect, it } from 'vitest';

import { evidenceHref, parseStructuredAnswer } from './answer';

describe('parseStructuredAnswer', () => {
  it('parses a fenced JSON answer', () => {
    const content = 'Here is the analysis:\n```json\n{"summary":"spike","confidence":0.8}\n```';
    const a = parseStructuredAnswer(content);
    expect(a?.summary).toBe('spike');
    expect(a?.confidence).toBe(0.8);
  });

  it('parses a bare JSON answer with answer keys', () => {
    const a = parseStructuredAnswer('{"likely_causes":["bad deploy"]}');
    expect(a?.likely_causes).toEqual(['bad deploy']);
  });

  it('returns null for plain text', () => {
    expect(parseStructuredAnswer('just a sentence')).toBeNull();
  });

  it('returns null for JSON without answer keys', () => {
    expect(parseStructuredAnswer('{"foo":1}')).toBeNull();
  });

  it('normalizes the product answer sections and ignores malformed evidence', () => {
    expect(
      parseStructuredAnswer(
        JSON.stringify({
          summary: '暂时无法确认当前值班人',
          evidence: ['internal tool name', { label: '没有匹配的有效排班' }],
          limitations: ['排班上下文不可用'],
          suggested_next_steps: ['选择排班后重试'],
        }),
      ),
    ).toEqual({
      summary: '暂时无法确认当前值班人',
      evidence: [{ label: '没有匹配的有效排班' }],
      limitations: ['排班上下文不可用'],
      suggested_next_steps: ['选择排班后重试'],
    });
  });
});

describe('evidenceHref', () => {
  it('routes logs evidence with time range and stream', () => {
    const href = evidenceHref({
      kind: 'logs',
      stream: 'app',
      time_range: { start_micros: 100, end_micros: 200 },
    });
    expect(href).toContain('/logs?');
    expect(href).toContain('from=100');
    expect(href).toContain('to=200');
    expect(href).toContain('stream=app');
  });

  it('routes a trace by id', () => {
    expect(evidenceHref({ kind: 'trace', trace_id: 'abc' })).toBe('/traces/abc');
  });

  it('honors an explicit route/href', () => {
    expect(evidenceHref({ route: '/alerts/incidents/1' })).toBe('/alerts/incidents/1');
  });

  it('returns null for non-navigable evidence (archive object key)', () => {
    expect(evidenceHref({ object_key: 'intelligence/chat/x.json' })).toBeNull();
  });
});

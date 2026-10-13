import type { TFunction } from 'i18next';
import { describe, expect, it } from 'vitest';

import type { LogEntry } from '../viewModel';
import { buildInspectionPrompt, digestLoadedLogs } from './model';

const log = (level: string, raw: Record<string, unknown>): LogEntry => ({ ts: '', micros: 1, level: level as LogEntry['level'], raw });
// The words are the product's; here a key stands for them, with what was put into it.
const t = ((key: string, values?: Record<string, unknown>) =>
  values ? `${key} ${JSON.stringify(values)}` : key) as unknown as TFunction<'logs'>;
const TARGET = { stream: 'app_logs', statement: "level = 'ERROR'", range: { start: 10, end: 20 } };

describe('digestLoadedLogs', () => {
  const rows = [
    log('INFO', { 'service.name': 'api', message: 'GET / 200' }),
    log('ERROR', { 'service.name': 'api', message: 'upstream timeout' }),
    log('ERROR', { service: 'gateway', message: 'upstream timeout' }),
    log('WARN', { 'service.name': 'api', body: 'card declined' }),
    log('ERROR', { 'service.name': 'api' }),
  ];

  it('says how the loaded logs divide, the commonest first', () => {
    const digest = digestLoadedLogs(rows);

    expect(digest.loaded).toBe(5);
    expect(digest.levels).toEqual([
      ['ERROR', 3],
      ['INFO', 1],
      ['WARN', 1],
    ]);
    expect(digest.services).toEqual([
      ['api', 4],
      ['gateway', 1],
    ]);
  });

  it('keeps the messages that are not routine, each once with how often it was met', () => {
    expect(digestLoadedLogs(rows).notable).toEqual([
      { level: 'ERROR', message: 'upstream timeout', count: 2 },
      { level: 'WARN', message: 'card declined', count: 1 },
    ]);
  });

  it('cuts a long message and keeps no more than a prompt has room for', () => {
    const many = Array.from({ length: 30 }, (_, index) => log('ERROR', { message: `${'x'.repeat(400)} ${index}` }));
    const digest = digestLoadedLogs(many);

    // Past their first 240 characters the thirty are one message.
    expect(digest.notable).toEqual([{ level: 'ERROR', message: 'x'.repeat(240), count: 30 }]);
    expect(digestLoadedLogs(Array.from({ length: 30 }, (_, index) => log('ERROR', { message: `m${index}` }))).notable).toHaveLength(8);
  });
});

describe('buildInspectionPrompt', () => {
  const digest = digestLoadedLogs([log('ERROR', { 'service.name': 'api', message: 'boom' })]);

  it('names the stream, the range, the query and what the page already says', () => {
    const prompt = buildInspectionPrompt(t, { target: TARGET, rangeLabel: 'Last 1 hour', requirements: '', digest });

    expect(prompt).toContain('explore.inspection.prompt.task {"stream":"app_logs"}');
    expect(prompt).toContain('explore.inspection.prompt.range {"label":"Last 1 hour","start":10,"end":20}');
    expect(prompt).toContain(`explore.inspection.prompt.query {"statement":"level = 'ERROR'"}`);
    expect(prompt).toContain('explore.inspection.prompt.digest {"count":1}');
    expect(prompt).toContain('- explore.inspection.prompt.digest_levels: ERROR 1');
    expect(prompt).toContain('- explore.inspection.prompt.digest_services: api 1');
    expect(prompt).toContain('- [ERROR ×1] boom');
    expect(prompt).not.toContain('explore.inspection.prompt.requirements');
  });

  it('asks for every log where the query is empty, and adds what the reader asked for', () => {
    const prompt = buildInspectionPrompt(t, {
      target: { ...TARGET, statement: '  ' },
      rangeLabel: 'Last 1 hour',
      requirements: ' only payment-service ',
      digest: digestLoadedLogs([]),
    });

    expect(prompt).toContain('explore.inspection.prompt.query_all');
    expect(prompt).not.toContain('explore.inspection.prompt.digest');
    expect(prompt.endsWith('explore.inspection.prompt.requirements\nonly payment-service')).toBe(true);
  });
});

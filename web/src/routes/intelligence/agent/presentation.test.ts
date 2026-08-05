import { describe, expect, it } from 'vitest';

import {
  fallbackToolLabel,
  formatInvestigationDuration,
  isRedundantInvestigationSummary,
  parseInvestigationEvidence,
  sanitizeAssistantContent,
} from './presentation';

describe('Mole Agent investigation presentation', () => {
  it('parses compact persisted tool evidence', () => {
    expect(
      parseInvestigationEvidence([
        {
          tool_call_id: 'call-1',
          tool: 'list_recent_alerts',
          status: 'success',
          summary: '0 rows',
          row_count: 0,
          took_ms: 64,
          arguments: { severity: 'critical' },
        },
      ]),
    ).toEqual([
      {
        toolCallId: 'call-1',
        tool: 'list_recent_alerts',
        status: 'success',
        summary: '0 rows',
        rowCount: 0,
        tookMs: 64,
        arguments: { severity: 'critical' },
      },
    ]);
  });

  it('ignores malformed entries and creates readable fallback labels', () => {
    expect(parseInvestigationEvidence([null, {}, { tool: 'query_logs' }])).toHaveLength(1);
    expect(fallbackToolLabel('get_current_on_call')).toBe('Current On Call');
  });

  it('formats compact Codex-style investigation durations', () => {
    expect(formatInvestigationDuration(320)).toBe('<1s');
    expect(formatInvestigationDuration(5_000)).toBe('5s');
    expect(formatInvestigationDuration(302_000)).toBe('5m 2s');
    expect(formatInvestigationDuration(3_661_000)).toBe('1h 1m 1s');
    expect(formatInvestigationDuration(Number.NaN)).toBe('');
  });

  it('hides summaries that only repeat the localized row count', () => {
    expect(isRedundantInvestigationSummary('7 rows', 7)).toBe(true);
    expect(isRedundantInvestigationSummary('1 row', 1)).toBe(true);
    expect(isRedundantInvestigationSummary('返回 4 条', 4)).toBe(true);
    expect(isRedundantInvestigationSummary('query failed', 0)).toBe(false);
  });

  it('hides complete and streaming DSML tool-call transport', () => {
    const complete = [
      '我先检查最近的日志。',
      '<|DSML|tool_calls>',
      '<|DSML|invoke name="query_logs">',
      '<|DSML|parameter name="sql" string="true">SELECT * FROM logs<|DSML|parameter>',
      '</|DSML|invoke>',
      '</|DSML|tool_calls>',
      '暂未发现异常。',
    ].join('\n');
    expect(sanitizeAssistantContent(complete)).toBe(
      '我先检查最近的日志。\n\n暂未发现异常。',
    );

    expect(
      sanitizeAssistantContent(
        '正在查询。\n<|DSML|tool_calls>\n<|DSML|invoke name="query_logs">',
      ),
    ).toBe('正在查询。');
  });

  it('hides DSML transport written with full-width or repeated separators', () => {
    const fullWidth = [
      '现在我有具体数据了。',
      '<｜｜DSML｜｜tool_calls>',
      '<｜｜DSML｜｜invoke name="query_traces">',
      '<｜｜DSML｜｜parameter name="sql" string="true">SELECT * FROM traces</｜｜DSML｜｜parameter>',
      '</｜｜DSML｜｜invoke>',
      '</｜｜DSML｜｜tool_calls>',
      '根因是 payments 服务返回错误。',
    ].join('\n');

    expect(sanitizeAssistantContent(fullWidth)).toBe(
      '现在我有具体数据了。\n\n根因是 payments 服务返回错误。',
    );

    expect(
      sanitizeAssistantContent(
        '正在查询。\n<｜DSML｜tool_calls>\n<｜｜DSML｜｜invoke name="query_logs">',
      ),
    ).toBe('正在查询。');
  });
});

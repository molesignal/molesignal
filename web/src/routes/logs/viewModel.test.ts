import { describe, expect, it } from 'vitest';

import {
  defaultLogTableFields,
  logSourceLabel,
  primaryLogMessage,
  recordsToCsv,
  recordsToLogText,
  topLogFieldValues,
} from './viewModel';

describe('log view model', () => {
  it('chooses a readable primary message and source', () => {
    const record = {
      'service.name': 'checkout-api',
      body: 'payment request failed',
      status_code: 500,
    };

    expect(primaryLogMessage(record)).toEqual({
      field: 'body',
      value: 'payment request failed',
    });
    expect(logSourceLabel(record)).toBe('checkout-api');
  });

  it('keeps useful table fields first and excludes duplicate timestamp columns', () => {
    expect(defaultLogTableFields([
      '_timestamp',
      'completion_tokens',
      'provider',
      'model',
      'error',
    ])).toEqual(['model', 'provider', 'error', 'completion_tokens']);
  });

  it('counts top values deterministically', () => {
    expect(topLogFieldValues([
      { provider: 'deepseek' },
      { provider: 'openai' },
      { provider: 'deepseek' },
    ], 'provider')).toEqual([
      { value: 'deepseek', label: 'deepseek', count: 2 },
      { value: 'openai', label: 'openai', count: 1 },
    ]);
  });

  it('escapes CSV cells', () => {
    expect(recordsToCsv([{ message: 'hello, "world"' }], ['message'])).toBe(
      '"message"\n"hello, ""world"""',
    );
  });

  it('formats ordered fields as one readable log record per line', () => {
    expect(recordsToLogText([
      {
        _timestamp: '2026-08-02T18:32:12Z',
        level: 'INFO',
        service: { name: 'checkout api' },
        message: 'hello "world"\nnext',
      },
    ], ['_timestamp', 'message', 'level', 'service.name'])).toBe(
      '2026-08-02T18:32:12Z message="hello \\"world\\"\\nnext" level=INFO service.name="checkout api"',
    );
  });
});

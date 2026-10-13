import { describe, expect, it } from 'vitest';

import {
  captureLogFieldPosition,
  defaultLogTableFields,
  formatLogMoment,
  type LogClock,
  logLevelTone,
  logSource,
  logSourceLabel,
  logStatement,
  momentsShareDay,
  primaryLogMessage,
  recordsToCsv,
  recordsToLogText,
  restoreLogFieldPosition,
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

  describe('the columns a stream opens with', () => {
    const fields = (...names: string[]) => names.map((name) => ({ name, type: 'string' }));

    it('are what a reader looks for first, found among the fields the stream has: five with the time', () => {
      expect(defaultLogTableFields(fields(
        '_timestamp',
        'http.route',
        'trace_id',
        'message',
        'k8s.pod.name',
        'level',
        'service.name',
        'caller',
        'duration_ms',
      ))).toEqual(['level', 'service.name', 'message', 'trace_id']);
    });

    it('know a meaning by the names streams give it', () => {
      expect(defaultLogTableFields(fields('_timestamp', 'severity_text', 'appName', 'msg', 'traceId')))
        .toEqual(['severity_text', 'appName', 'msg', 'traceId']);
      // A name that ends in the meaning is that meaning too, after one that is named for it outright.
      expect(defaultLogTableFields(fields('_timestamp', 'log.level', 'k8s.pod.name', 'body', 'log.logger')))
        .toEqual(['log.level', 'body', 'log.logger', 'k8s.pod.name']);
      expect(defaultLogTableFields(fields('_timestamp', 'log.level', 'level', 'message'))[0]).toBe('level');
    });

    it('try the next of the common ones where the stream has none of one', () => {
      expect(defaultLogTableFields(fields('_timestamp', 'level', 'service', 'message', 'class', 'caller')))
        .toEqual(['level', 'service', 'message', 'class']);
      expect(defaultLogTableFields(fields('_timestamp', 'level', 'service', 'message', 'caller', 'host')))
        .toEqual(['level', 'service', 'message', 'caller']);
    });

    it('are filled up with the fields the stream does have, in its order', () => {
      // Nothing of this stream is known by meaning: it is shown by what it writes.
      expect(defaultLogTableFields(fields(
        '_timestamp',
        'remote_addr',
        'request',
        'status',
        'body_bytes_sent',
        'http_referer',
      ))).toEqual(['remote_addr', 'request', 'status', 'body_bytes_sent']);
      expect(defaultLogTableFields(fields('_timestamp', 'level', 'message', 'user_id', 'duration_ms')))
        .toEqual(['level', 'message', 'user_id', 'duration_ms']);
    });

    it('do not open on a flag, a structure or a field of the store itself', () => {
      expect(defaultLogTableFields([
        { name: '_timestamp', type: 'timestamp' },
        { name: '_event_id', type: 'string' },
        { name: 'level', type: 'string' },
        { name: 'service', type: 'string' },
        { name: 'message', type: 'string' },
        { name: 'error', type: 'boolean' },
        { name: 'kubernetes', type: 'object' },
        { name: 'tags', type: 'array' },
      ])).toEqual(['level', 'service', 'message']);
    });

    it('list nothing the stream does not write, and no time beside the time', () => {
      expect(defaultLogTableFields(fields('_timestamp', 'timestamp', 'time'))).toEqual([]);
      expect(defaultLogTableFields([])).toEqual([]);
    });

    it('are as many as asked for', () => {
      expect(defaultLogTableFields(fields('level', 'service', 'message', 'trace_id', 'class', 'caller'), 6))
        .toEqual(['level', 'service', 'message', 'trace_id', 'class', 'caller']);
    });
  });

  it('reads the tone of a level from how it begins', () => {
    for (const level of ['ERROR', 'error', 'ERR', 'Fatal', 'CRITICAL', 'panic']) expect(logLevelTone(level)).toBe('error');
    for (const level of ['WARN', 'warning']) expect(logLevelTone(level)).toBe('warn');
    for (const level of ['DEBUG', 'trace', 'dbg']) expect(logLevelTone(level)).toBe('quiet');
    for (const level of ['INFO', 'notice', '', 'audit']) expect(logLevelTone(level)).toBe('info');
  });

  it('writes a moment to the millisecond, with as much of the day as is asked for', () => {
    const utc: LogClock = { tz: 'UTC', format: 'iso_24h', dateFormat: 'yyyy_mm_dd_dash' };
    const moment = Date.parse('2026-05-23T09:59:54.120Z') * 1_000;

    expect(formatLogMoment(moment, utc, 'time')).toBe('09:59:54.120');
    expect(formatLogMoment(moment, utc, 'day')).toBe('05-23 09:59:54.120');
    expect(formatLogMoment(moment, utc, 'full')).toBe('2026-05-23 09:59:54.120');
    // In the reader's zone and format, not the browser's.
    expect(
      formatLogMoment(moment, { tz: 'Asia/Shanghai', format: 'local_12h', dateFormat: 'dd_mm_yyyy_slash' }, 'full'),
    ).toBe('23/05/2026 05:59:54.120 PM');
  });

  it('says the day on every row only where the rows are of more than one', () => {
    const at = (iso: string) => Date.parse(iso) * 1_000;

    expect(momentsShareDay([at('2026-05-23T00:00:01Z'), at('2026-05-23T23:59:59Z'), null], 'UTC')).toBe(true);
    expect(momentsShareDay([at('2026-05-23T23:59:59Z'), at('2026-05-24T00:00:01Z')], 'UTC')).toBe(false);
    // The same two instants are one day further east.
    expect(momentsShareDay([at('2026-05-23T23:59:59Z'), at('2026-05-24T00:00:01Z')], 'Asia/Shanghai')).toBe(true);
    expect(momentsShareDay([], 'UTC')).toBe(true);
  });

  it('says where a log came from as a field, so its value can be followed', () => {
    expect(logSource({ 'service.name': 'checkout-api', host: 'ip-10-0-0-1' })).toEqual({
      field: 'service.name',
      value: 'checkout-api',
    });
    expect(logSource({ message: 'hello' })).toBeNull();
  });

  it('restores a hidden field to its original table position', () => {
    const initial = ['_timestamp', 'level', 'service.name', 'message'];
    const position = captureLogFieldPosition(initial, 'level');
    const hidden = initial.filter((field) => field !== 'level');

    expect(restoreLogFieldPosition(hidden, 'level', position)).toEqual(initial);
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

describe('logStatement', () => {
  it('is the message, with the field it is in', () => {
    expect(logStatement({ level: 'ERROR', message: 'failed to reserve stock' })).toEqual({
      field: 'message',
      value: 'failed to reserve stock',
    });
    expect(logStatement({ body: 'request completed', level: 'INFO' })).toEqual({
      field: 'body',
      value: 'request completed',
    });
  });

  it('does not pass off another field as a message', () => {
    // A record of a model call: its first field is not what it says.
    expect(logStatement({ model: 'gpt', provider: 'openai', completion_tokens: 12 })).toBeNull();
    expect(logStatement({ message: '   ' })).toBeNull();
    expect(logStatement({ message: { nested: true } })).toBeNull();
  });
});

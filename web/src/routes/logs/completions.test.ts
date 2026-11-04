import { describe, expect, it } from 'vitest';

import type { StreamField } from '@/api/streams';
import type { CodeCompletionItem } from '@/shell/codeEditor/types';
import { resolveSqlFunctionCompletions } from '@/shell/query/useSqlFunctionCompletions';

import { logQueryCompletions } from './completions';
import { deriveLogFields } from './fieldQueryModel';

function streamField(name: string, dataType: StreamField['dataType'] = 'utf8'): StreamField {
  return { name, dataType: dataType, nullable: true, indexed: false };
}

// A stream OTLP wrote: none of its fields is called `level`, `service` or `status`.
const fields = deriveLogFields([
  streamField('service.name'),
  streamField('severity_text'),
  streamField('body'),
  streamField('http.status_code', 'int64'),
]);
const rows = [
  { raw: { 'service.name': 'checkout', severity_text: 'error', body: 'payment declined', 'http.status_code': 502 } },
  { raw: { 'service.name': 'cart', severity_text: 'info', body: 'item added', 'http.status_code': 200 } },
];
const functions = resolveSqlFunctionCompletions(undefined);
const NOTHING_MASKED = new Set<string>();

function labels(items: CodeCompletionItem[], kind: CodeCompletionItem['kind']): string[] {
  return items.filter((item) => item.kind === kind).map((item) => item.label);
}

function valuesOf(items: CodeCompletionItem[], field: string): (string | undefined)[] {
  return items.filter((item) => item.kind === 'value' && item.field === field).map((item) => item.value);
}

describe('log query completions', () => {
  it('offer the fields of the stream and no other', () => {
    const items = logQueryCompletions(functions, fields, rows, NOTHING_MASKED);

    expect(labels(items, 'field')).toEqual([
      '_timestamp',
      'body',
      'http.status_code',
      'service.name',
      'severity_text',
    ]);
  });

  it('offer the values the loaded logs hold, as they are written', () => {
    const items = logQueryCompletions(functions, fields, rows, NOTHING_MASKED);

    expect(valuesOf(items, 'service.name')).toEqual(['cart', 'checkout']);
    expect(valuesOf(items, 'severity_text')).toEqual(['error', 'info']);
    expect(valuesOf(items, 'http.status_code')).toEqual(['200', '502']);
    expect(items).toContainEqual({
      label: '"checkout"',
      insertText: '"checkout"',
      kind: 'value',
      detail: 'service.name value',
      field: 'service.name',
      value: 'checkout',
    });
    expect(new Set(items.filter((item) => item.kind === 'value').map((item) => item.field)))
      .toEqual(new Set(['service.name', 'severity_text', 'body', 'http.status_code']));
  });

  it('offer no value too long to compare with, nor one that is no plain value', () => {
    const items = logQueryCompletions(
      functions,
      deriveLogFields([streamField('body'), streamField('attributes', 'json'), streamField('retried', 'bool')]),
      [{ raw: { body: 'x'.repeat(81), attributes: { user: 'u-1' }, retried: false } }, { raw: { body: '' } }],
      NOTHING_MASKED,
    );

    expect(valuesOf(items, 'body')).toEqual([]);
    expect(valuesOf(items, 'attributes')).toEqual([]);
    expect(valuesOf(items, 'retried')).toEqual(['false']);
  });

  it('make up no field and no value while the stream has none', () => {
    const items = logQueryCompletions(
      functions,
      [],
      [{ raw: { level: 'ERROR', service: 'checkout' } }],
      NOTHING_MASKED,
    );

    expect(labels(items, 'field')).toEqual([]);
    expect(labels(items, 'value')).toEqual([]);
  });

  it('offer no value of a masked field, whatever case the stream writes it in', () => {
    const items = logQueryCompletions(
      functions,
      deriveLogFields([streamField('service.name'), streamField('Client.IP')]),
      [{ raw: { 'service.name': 'checkout', 'Client.IP': '10.0.0.7' } }],
      new Set(['client.ip']),
    );

    expect(labels(items, 'field')).toContain('Client.IP');
    expect(valuesOf(items, 'Client.IP')).toEqual([]);
    expect(valuesOf(items, 'service.name')).toEqual(['checkout']);
  });

  it('offer no value before the masking of the stream is known', () => {
    const items = logQueryCompletions(functions, fields, rows, null);

    expect(labels(items, 'field')).toContain('service.name');
    expect(labels(items, 'value')).toEqual([]);
  });

  it('keep the search functions and the comparisons', () => {
    const items = logQueryCompletions(functions, fields, rows, NOTHING_MASKED);

    expect(items.slice(0, functions.length)).toEqual(functions);
    expect(labels(items, 'function')).toEqual(['MATCH', 'MATCH_TEXT', 'CIDR_MATCH']);
    expect(labels(items, 'operator')).toEqual(['=', '!=', '>=', '<=', '>', '<', 'contains', 'AND', 'OR']);
    expect(labels(logQueryCompletions(functions, [], [], null), 'operator')).toEqual(labels(items, 'operator'));
  });
});

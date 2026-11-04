import { describe, expect, it } from 'vitest';

import { layOutJson, offsetInLayout } from './jsonLayout';

describe('layOutJson', () => {
  it('lays a text out as JSON.stringify does with two-space indents', () => {
    const texts = [
      '{"alert_id":"incident-preview","value":512,"labels":{"service":"api","env":"production"}}',
      '[1,2,[3,{"a":null}],{"b":[true,false]}]',
      '{ "empty": {}, "none": [ ], "nested": { "deep": [ { } ] } }',
      '{"text":"a, b: {c} [d]","quote":"say \\"hi\\"","slash":"\\\\","unicode":"中文"}',
      '\n\t{\r\n  "spaced"  :   1.5 ,\n"negative":-2}\n',
    ];

    for (const text of texts) {
      expect(layOutJson(text)).toBe(JSON.stringify(JSON.parse(text), null, 2));
    }
  });

  it('keeps every token as it is written: digits, keys in their places, a key written twice', () => {
    expect(layOutJson('{"id":12345678901234567890,"2":"b","1":"a","1":"again"}')).toBe(
      '{\n  "id": 12345678901234567890,\n  "2": "b",\n  "1": "a",\n  "1": "again"\n}',
    );
    expect(layOutJson('{"ratio":1.5e-3,"zero":-0,"e":"\\u00e9"}')).toBe(
      '{\n  "ratio": 1.5e-3,\n  "zero": -0,\n  "e": "\\u00e9"\n}',
    );
  });

  it('leaves a text that is laid out as it is, and a value on its own', () => {
    const laidOut = '{\n  "labels": {\n    "service": "api"\n  },\n  "tags": []\n}';

    expect(layOutJson(laidOut)).toBe(laidOut);
    expect(layOutJson(' "text" ')).toBe('"text"');
    expect(layOutJson('42')).toBe('42');
    expect(layOutJson('null')).toBe('null');
  });

  it('says a text that is not JSON is none', () => {
    for (const text of ['', '   ', '{', '{"a":}', '***', "{'a': 1}", '{"a": 1,}', '[1] [2]']) {
      expect(layOutJson(text)).toBeNull();
    }
  });
});

describe('offsetInLayout', () => {
  const text = '{"name":"api gateway","tags":["a","b"]}';
  const laidOut = layOutJson(text)!;

  it('keeps the caret after the same characters', () => {
    // After the pasted text, the caret is after all of it.
    expect(offsetInLayout(text, text.length, laidOut)).toBe(laidOut.length);
    expect(offsetInLayout(text, 0, laidOut)).toBe(0);

    // In the middle of a string, its space counted.
    const inString = text.indexOf('gateway');
    expect(laidOut.slice(0, offsetInLayout(text, inString, laidOut))).toBe('{\n  "name": "api ');

    // After a comma, before the line break the layout puts there.
    const afterComma = text.indexOf(',"tags"') + 1;
    expect(laidOut.slice(0, offsetInLayout(text, afterComma, laidOut))).toBe('{\n  "name": "api gateway",');
  });

  it('counts an escaped quote as part of its string', () => {
    const escaped = '{"say":"\\"hi\\"","n":1}';
    const escapedLaidOut = layOutJson(escaped)!;
    const beforeOne = escaped.indexOf('1');

    expect(escapedLaidOut.slice(0, offsetInLayout(escaped, beforeOne, escapedLaidOut))).toBe(
      '{\n  "say": "\\"hi\\"",\n  "n":',
    );
  });
});

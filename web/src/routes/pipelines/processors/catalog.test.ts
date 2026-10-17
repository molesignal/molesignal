import { describe, expect, it } from 'vitest';

import {
  compileProcessor,
  defaultProcessor,
  parseProcessor,
  PROCESSOR_OPS,
  processorProblem,
  processorStepName,
  processorSummary,
  type ProcessorConfig,
} from './catalog';

/**
 * Every script here was run on the product's VRL runtime with an event that
 * has the field, one that lacks it and one where it is of another type. A
 * change to what is generated should be run there again before it is pinned.
 */
describe('compileProcessor', () => {
  const vrl = (config: ProcessorConfig) => compileProcessor(config).split('\n');

  it('merges parsed JSON into the event, dropping the source first so a parsed key of that name survives', () => {
    expect(vrl({ op: 'parse_json', source: 'message', target: '', removeSource: true })).toEqual([
      'parsed, err = parse_json(.message)',
      'if err == null && is_object(parsed) {',
      '  del(.message)',
      '  . = merge(., object!(parsed))',
      '}',
    ]);
  });

  it('puts parsed JSON under a field when one is named', () => {
    expect(vrl({ op: 'parse_json', source: 'message', target: 'payload.data', removeSource: false })).toEqual([
      'parsed, err = parse_json(.message)',
      'if err == null {',
      '  .payload.data = parsed',
      '}',
    ]);
  });

  it('parses key=value text', () => {
    expect(vrl({ op: 'parse_key_value', source: 'message', target: '', removeSource: false })).toEqual([
      'parsed, err = parse_key_value(.message)',
      'if err == null {',
      '  . = merge(., parsed)',
      '}',
    ]);
  });

  it('extracts named groups with a regex', () => {
    expect(vrl({ op: 'parse_regex', source: 'message', pattern: '^(?P<status>\\d+)$' })).toEqual([
      "parsed, err = parse_regex(.message, r'^(?P<status>\\d+)$')",
      'if err == null {',
      '  . = merge(., parsed)',
      '}',
    ]);
  });

  it('looks for a dotted source as a key, then as a path', () => {
    expect(vrl({ op: 'parse_json', source: 'log.body', target: '', removeSource: false })).toEqual([
      'if exists(."log.body") {',
      '  parsed, err = parse_json(."log.body")',
      '  if err == null && is_object(parsed) {',
      '    . = merge(., object!(parsed))',
      '  }',
      '} else if exists(.log.body) {',
      '  parsed, err = parse_json(.log.body)',
      '  if err == null && is_object(parsed) {',
      '    . = merge(., object!(parsed))',
      '  }',
      '}',
    ]);
  });

  it('renames a field that is there', () => {
    expect(vrl({ op: 'rename', from: 'msg', to: 'message' })).toEqual([
      'if exists(.msg) {',
      '  .message = del(.msg)',
      '}',
    ]);
  });

  it('removes every way a listed field may be held, and skips blanks', () => {
    expect(vrl({ op: 'remove', fields: ['password', ' ', 'user.token'] })).toEqual([
      'del(.password)',
      'del(."user.token")',
      'del(.user.token)',
    ]);
  });

  it('sets a text, a number, a boolean or a copy of another field', () => {
    const set = (valueType: 'string' | 'number' | 'boolean' | 'field', value: string) =>
      compileProcessor({ op: 'set', field: 'labels.team', valueType, value });

    expect(set('string', 'core "a" {{ x }}')).toBe('.labels.team = "core \\"a\\" \\{{ x }}"');
    expect(set('number', ' -0.25 ')).toBe('.labels.team = -0.25');
    expect(set('boolean', 'false')).toBe('.labels.team = false');
    expect(set('field', 'app')).toBe('if exists(.app) {\n  .labels.team = .app\n}');
  });

  it('masks whatever the value is, or keeps the end of a text, or hashes it', () => {
    expect(vrl({ op: 'mask', field: 'token', mode: 'full' })).toEqual([
      'if exists(.token) {',
      '  .token = "***"',
      '}',
    ]);
    expect(vrl({ op: 'mask', field: 'card', mode: 'last4' })).toEqual([
      'if is_string(.card) {',
      '  value = string!(.card)',
      '  .card = if length(value) > 4 { "****" + slice!(value, -4) } else { "****" }',
      '}',
    ]);
    expect(vrl({ op: 'mask', field: 'email', mode: 'hash' })).toEqual([
      'if is_string(.email) {',
      '  .email = sha2(string!(.email), variant: "SHA-256")',
      '}',
    ]);
  });

  it('changes the case of a text and leaves anything else alone', () => {
    expect(compileProcessor({ op: 'case', field: 'level', to: 'lower' })).toBe(
      'if is_string(.level) {\n  .level = downcase(string!(.level))\n}',
    );
    expect(compileProcessor({ op: 'case', field: 'level', to: 'upper' })).toContain('upcase(');
  });

  it('converts a value that is there, never one that is missing or null', () => {
    expect(vrl({ op: 'convert', field: 'status', to: 'integer' })).toEqual([
      'if exists(.status) {',
      '  converted, err = to_int(.status)',
      '  if err == null && .status != null {',
      '    .status = converted',
      '  }',
      '}',
    ]);
    expect(compileProcessor({ op: 'convert', field: 'n', to: 'float' })).toContain('to_float(.n)');
    expect(compileProcessor({ op: 'convert', field: 'n', to: 'boolean' })).toContain('to_bool(.n)');
    expect(compileProcessor({ op: 'convert', field: 'n', to: 'string' })).toContain('to_string(.n)');
  });

  // These scripts are also pinned, and run, in the VRL runtime's own test
  // (`the_editors_filter_and_sample_programs_drop_what_they_say`).
  it('keeps the events a filter matches and aborts the rest', () => {
    expect(vrl({ op: 'filter', action: 'keep', field: 'level', test: 'equals', value: 'error' })).toEqual([
      'value = .level',
      'matched = value == "error"',
      'if !matched {',
      '  abort',
      '}',
    ]);
  });

  it('compares numbers as numbers, text that reads as one included', () => {
    expect(vrl({ op: 'filter', action: 'drop', field: 'status', test: 'gte', value: '500' })).toEqual([
      'value = .status',
      'matched = false',
      'if value != null {',
      '  number, err = to_float(value)',
      '  matched = err == null && number >= 500.0',
      '}',
      'if matched {',
      '  abort',
      '}',
    ]);
    // A value typed as a number equals the number and its text.
    expect(compileProcessor({ op: 'filter', action: 'drop', field: 'code', test: 'not_equals', value: '200' })).toContain(
      'matched = !(value == 200 || value == "200")',
    );
  });

  it('looks for a dotted field as a key, then as a path, and tests text only as text', () => {
    expect(
      vrl({ op: 'filter', action: 'drop', field: 'service.name', test: 'matches', value: '^test-' }),
    ).toEqual([
      'value = null',
      'if exists(."service.name") {',
      '  value = ."service.name"',
      '} else if exists(.service.name) {',
      '  value = .service.name',
      '}',
      "matched = is_string(value) && match(string!(value), r'^test-')",
      'if matched {',
      '  abort',
      '}',
    ]);
    expect(vrl({ op: 'filter', action: 'keep', field: 'user.id', test: 'exists', value: '' })).toEqual([
      'matched = exists(."user.id") || exists(.user.id)',
      'if !matched {',
      '  abort',
      '}',
    ]);
    expect(compileProcessor({ op: 'filter', action: 'keep', field: 'message', test: 'contains', value: 'time"out' })).toContain(
      'matched = is_string(value) && contains(string!(value), "time\\"out")',
    );
  });

  it('samples at random, or by a field so that one value is kept or dropped whole', () => {
    expect(vrl({ op: 'sample', percent: '10', by: '' })).toEqual([
      'if !(random_float(0.0, 100.0) < 10.0) {',
      '  abort',
      '}',
    ]);
    expect(vrl({ op: 'sample', percent: '25', by: 'trace_id' })).toEqual([
      'value = .trace_id',
      'key, err = to_string(value)',
      'sampled = false',
      'if value == null || err != null {',
      '  sampled = random_float(0.0, 100.0) < 25.0',
      '} else {',
      '  sampled = mod(abs(seahash(key)), 10000) < 2500',
      '}',
      'if !sampled {',
      '  abort',
      '}',
    ]);
    expect(compileProcessor({ op: 'sample', percent: '0.5', by: 'id' })).toContain('< 50\n');
  });
});

describe('processors', () => {
  it('start incomplete where they need a field, and say what they lack', () => {
    const problems = Object.fromEntries(
      PROCESSOR_OPS.map((op) => [op, processorProblem(defaultProcessor(op))]),
    );

    expect(problems).toEqual({
      // `message` is where text to parse usually is.
      parse_json: null,
      parse_key_value: null,
      parse_regex: 'processor_pattern_missing',
      rename: 'processor_field_missing',
      remove: 'processor_field_missing',
      set: 'processor_field_missing',
      mask: 'processor_field_missing',
      case: 'processor_field_missing',
      convert: 'processor_field_missing',
      filter: 'processor_field_missing',
      // A tenth of the events, picked at random.
      sample: null,
    });
  });

  it('are complete once their fields are named', () => {
    const complete: ProcessorConfig[] = [
      { op: 'parse_regex', source: 'message', pattern: '(?P<a>x)' },
      { op: 'rename', from: 'a', to: 'b' },
      { op: 'remove', fields: ['a'] },
      { op: 'set', field: 'a', valueType: 'string', value: '' },
      { op: 'set', field: 'a', valueType: 'number', value: '12.5' },
      { op: 'set', field: 'a', valueType: 'field', value: 'b' },
      { op: 'mask', field: 'a', mode: 'hash' },
      { op: 'case', field: 'a', to: 'upper' },
      { op: 'convert', field: 'a', to: 'string' },
      { op: 'filter', action: 'keep', field: 'a', test: 'equals', value: 'b' },
      // Whether a field is there needs no value.
      { op: 'filter', action: 'drop', field: 'a', test: 'missing', value: '' },
      { op: 'filter', action: 'drop', field: 'a', test: 'lt', value: '-2.5' },
      { op: 'sample', percent: '100', by: '' },
      { op: 'sample', percent: '0.1', by: 'trace_id' },
    ];

    for (const config of complete) expect(processorProblem(config), config.op).toBeNull();
  });

  it('say what a filter or a sample still lacks, or has wrong', () => {
    const filter = (test: 'equals' | 'matches' | 'gt', value: string): ProcessorConfig => ({
      op: 'filter',
      action: 'keep',
      field: 'a',
      test,
      value,
    });

    expect(processorProblem(filter('equals', ' '))).toBe('processor_field_missing');
    expect(processorProblem(filter('matches', ''))).toBe('processor_pattern_missing');
    expect(processorProblem(filter('gt', 'many'))).toBe('processor_number_invalid');
    for (const percent of ['0', '101', '-5', 'ten', '']) {
      expect(processorProblem({ op: 'sample', percent, by: '' }), percent).toBe('processor_percent_invalid');
    }
  });

  it('refuse a number that is not one', () => {
    for (const value of ['abc', '1e3', '1.', '--1']) {
      expect(processorProblem({ op: 'set', field: 'a', valueType: 'number', value })).toBe(
        'processor_number_invalid',
      );
    }
    // Nothing typed is a field left empty, not a number written wrongly.
    expect(processorProblem({ op: 'set', field: 'a', valueType: 'number', value: ' ' })).toBe(
      'processor_field_missing',
    );
  });

  it('name a new step after what it does', () => {
    expect(processorStepName('parse_key_value')).toBe('parse-key-value');
    expect(processorStepName('mask')).toBe('mask');
  });

  it('say in a few words which fields they touch', () => {
    expect(processorSummary({ op: 'rename', from: 'msg', to: 'message' })).toBe('msg → message');
    expect(processorSummary({ op: 'rename', from: '', to: '' })).toBe('');
    expect(processorSummary({ op: 'remove', fields: ['a', '', 'b'] })).toBe('a, b');
    expect(processorSummary({ op: 'parse_json', source: 'message', target: '', removeSource: true })).toBe(
      'message',
    );
    expect(processorSummary({ op: 'convert', field: 'status', to: 'integer' })).toBe('status → integer');
    expect(processorSummary({ op: 'filter', action: 'drop', field: 'level', test: 'equals', value: 'debug' })).toBe(
      'level = debug',
    );
    expect(processorSummary({ op: 'filter', action: 'keep', field: 'user.id', test: 'exists', value: 'x' })).toBe(
      'user.id ∃',
    );
    expect(processorSummary({ op: 'sample', percent: '10', by: '' })).toBe('10%');
    expect(processorSummary({ op: 'sample', percent: '5', by: 'trace_id' })).toBe('5% · trace_id');
  });

  it('read back what they stored and nothing else', () => {
    for (const op of PROCESSOR_OPS) {
      const config = defaultProcessor(op);
      expect(parseProcessor(JSON.parse(JSON.stringify(config))), op).toEqual(config);
    }

    expect(parseProcessor(null)).toBeNull();
    expect(parseProcessor('rename')).toBeNull();
    expect(parseProcessor({ op: 'drop', field: 'a' })).toBeNull();
    expect(parseProcessor({ op: 'rename', from: 'a' })).toBeNull();
    expect(parseProcessor({ op: 'mask', field: 'a', mode: 'blur' })).toBeNull();
    expect(parseProcessor({ op: 'remove', fields: ['a', 1] })).toBeNull();
    expect(parseProcessor({ op: 'filter', action: 'skip', field: 'a', test: 'equals', value: 'b' })).toBeNull();
    expect(parseProcessor({ op: 'filter', action: 'keep', field: 'a', test: 'like', value: 'b' })).toBeNull();
    expect(parseProcessor({ op: 'sample', percent: 10, by: '' })).toBeNull();
    // Whatever else rode along is dropped.
    expect(parseProcessor({ op: 'rename', from: 'a', to: 'b', extra: true })).toEqual({
      op: 'rename',
      from: 'a',
      to: 'b',
    });
  });
});

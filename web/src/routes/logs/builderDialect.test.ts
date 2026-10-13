import { describe, expect, it } from 'vitest';

import { makeClause, withClause } from '@/shell/query/builder/model';

import { logBuilderDialect, logBuilderFields } from './builderDialect';
import { buildLogFieldQuerySql, type LogFieldDef } from './fieldQueryModel';

function field(name: string, dataType: LogFieldDef['dataType']): LogFieldDef {
  return { name, dataType, type: 'string', count: '0', expandable: false };
}

describe('logBuilderDialect', () => {
  it('shows a field query as filters, search text and what is neither', () => {
    const parsed = logBuilderDialect.parse(
      "service = 'checkout' AND status >= 500 AND MATCH(message, 'timeout') AND disk full AND level = '' AND code >= ''",
    );

    expect(parsed.clauses).toEqual([
      { field: 'service', op: '=', value: 'checkout', quoted: true },
      { field: 'status', op: '>=', value: '500', quoted: false },
      { field: 'message', op: 'match', value: 'timeout', quoted: true },
      // Compared with nothing, a field is asked whether it has a value at all.
      { field: 'level', op: '=', value: '', quoted: true },
    ]);
    expect(parsed.text).toBe('disk full');
    // A comparison with nothing to compare to is kept as written, not dropped.
    expect(parsed.unsupported).toEqual(["code >= ''"]);
  });

  it('writes a filter on a field having no value back as it was typed', () => {
    const statement = "level = '' AND env != ''";

    expect(logBuilderDialect.serialize(logBuilderDialect.parse(statement))).toBe(statement);
  });

  it('writes filters back as the statement the Code editor reads', () => {
    const statement = "service = 'checkout' AND status >= 500 AND MATCH_TEXT(message, 'disk full') AND timeout";

    expect(logBuilderDialect.serialize(logBuilderDialect.parse(statement))).toBe(statement);
  });

  it('keeps a value with quotes and line breaks intact through a round trip', () => {
    const awkward = "it's\na \\ path";
    const written = logBuilderDialect.serialize({
      clauses: [makeClause({ name: 'message', kind: 'text' }, '=', awkward)],
      text: '',
      unsupported: [],
    });

    expect(logBuilderDialect.parse(written).clauses[0]?.value).toBe(awkward);
  });

  it('a filter added in the builder runs as the same SQL as the typed one', () => {
    const typed = "service = 'checkout' AND status >= 500";
    const built = logBuilderDialect.serialize(
      withClause(
        logBuilderDialect.parse("service = 'checkout'"),
        null,
        makeClause({ name: 'status', kind: 'number' }, '>=', '500'),
      ),
    );

    expect(buildLogFieldQuerySql('app', built, 50)).toBe(buildLogFieldQuerySql('app', typed, 50));
  });

  it('turns text typed as a comparison into a filter when it is committed', () => {
    const parsed = logBuilderDialect.parse(
      logBuilderDialect.serialize({ clauses: [], text: 'level = error', unsupported: [] }),
    );

    expect(parsed.clauses).toEqual([{ field: 'level', op: '=', value: 'error', quoted: false }]);
    expect(parsed.text).toBe('');
  });

  it('offers comparisons by the kind of field and leaves JSON columns out', () => {
    const fields = logBuilderFields([
      field('service', 'utf8'),
      field('status', 'int64'),
      field('sampled', 'bool'),
      field('attributes', 'json'),
    ]);

    expect(fields.map((item) => item.name)).toEqual(['service', 'status', 'sampled']);
    expect(logBuilderDialect.operators(fields[0])).toContain('match_text');
    expect(logBuilderDialect.operators(fields[1])).toEqual(['=', '!=', '>', '>=', '<', '<=']);
    expect(logBuilderDialect.operators(fields[2])).toEqual(['=', '!=']);
    expect(logBuilderDialect.operators(undefined)).toContain('>=');
  });
});
